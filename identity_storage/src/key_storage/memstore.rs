// Copyright 2020-2023 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use core::fmt::Debug;
use std::collections::HashMap;
use std::fmt::Display;
use std::str::FromStr;

use async_trait::async_trait;
use crypto::signatures::ed25519::SecretKey;
use identity_jose::jwk::EdCurve;
use identity_jose::jwk::Jwk;
use identity_jose::jwk::JwkType;
use identity_jose::jws::JwsAlgorithm;
use rand::distributions::DistString;
use shared::Shared;
use tokio::sync::RwLockReadGuard;
use tokio::sync::RwLockWriteGuard;

use super::key_gen::JwkGenOutput;
use super::KeyId;
use super::KeyStorageError;
use super::KeyStorageErrorKind;
use super::KeyStorageResult;
use super::KeyType;
use crate::key_storage::JwkStorage;

/// The map from key ids to JWKs.
type JwkKeyStore = HashMap<KeyId, Jwk>;

/// An insecure, in-memory [`JwkStorage`] implementation that serves as an example and may be used in tests.
#[derive(Debug)]
pub struct JwkMemStore {
  jwk_store: Shared<JwkKeyStore>,
}

impl JwkMemStore {
  /// Creates a new, empty `JwkMemStore` instance.
  pub fn new() -> Self {
    Self {
      jwk_store: Shared::new(HashMap::new()),
    }
  }
}

// Refer to the `JwkStorage` interface docs for high-level documentation of the individual methods.
#[cfg_attr(not(feature = "send-sync-storage"), async_trait(?Send))]
#[cfg_attr(feature = "send-sync-storage", async_trait)]
impl JwkStorage for JwkMemStore {
  async fn generate(&self, key_type: KeyType, alg: JwsAlgorithm) -> KeyStorageResult<JwkGenOutput> {
    let key_type: MemStoreKeyType = MemStoreKeyType::try_from(&key_type)?;

    check_key_alg_compatibility(key_type, alg)?;

    let (private_key, public_key) = match key_type {
      MemStoreKeyType::Ed25519 => {
        let private_key = SecretKey::generate()
          .map_err(|err| KeyStorageError::new(KeyStorageErrorKind::RetryableIOFailure).with_source(err))?;
        let public_key = private_key.public_key();
        (private_key, public_key)
      }
    };

    let kid: KeyId = random_key_id();

    let mut jwk: Jwk = ed25519::encode_jwk(&private_key, &public_key);
    jwk.set_alg(alg.name());
    let public_jwk: Jwk = jwk.to_public();

    let mut jwk_store: RwLockWriteGuard<'_, JwkKeyStore> = self.jwk_store.write().await;
    jwk_store.insert(kid.clone(), jwk);

    Ok(JwkGenOutput::new(kid, public_jwk))
  }

  async fn insert(&self, jwk: Jwk) -> KeyStorageResult<KeyId> {
    let key_type = MemStoreKeyType::try_from(&jwk)?;

    if !jwk.is_private() {
      return Err(
        KeyStorageError::new(KeyStorageErrorKind::Unspecified)
          .with_custom_message("expected a Jwk with all private key components set"),
      );
    }

    match jwk.alg() {
      Some(alg) => {
        let alg: JwsAlgorithm = JwsAlgorithm::from_str(alg)
          .map_err(|err| KeyStorageError::new(KeyStorageErrorKind::UnsupportedSignatureAlgorithm).with_source(err))?;
        check_key_alg_compatibility(key_type, alg)?;
      }
      None => {
        return Err(
          KeyStorageError::new(KeyStorageErrorKind::UnsupportedSignatureAlgorithm)
            .with_custom_message("expected a Jwk with an `alg` parameter"),
        );
      }
    }

    if jwk.alg().is_none() {
      return Err(
        KeyStorageError::new(KeyStorageErrorKind::UnsupportedSignatureAlgorithm)
          .with_custom_message("expected a Jwk with an `alg` parameter"),
      );
    }

    let key_id: KeyId = random_key_id();

    let mut jwk_store: RwLockWriteGuard<'_, JwkKeyStore> = self.jwk_store.write().await;

    jwk_store.insert(key_id.clone(), jwk);

    Ok(key_id)
  }

  async fn sign(&self, key_id: &KeyId, data: Vec<u8>) -> KeyStorageResult<Vec<u8>> {
    let jwk_store: RwLockReadGuard<'_, JwkKeyStore> = self.jwk_store.read().await;

    let jwk: &Jwk = jwk_store
      .get(key_id)
      .ok_or_else(|| KeyStorageError::new(KeyStorageErrorKind::KeyNotFound))?;

    let alg: JwsAlgorithm =
      JwsAlgorithm::from_str(jwk.alg().expect("we should only store Jwks that have an `alg` set"))
        .map_err(|err| KeyStorageError::new(KeyStorageErrorKind::UnsupportedSignatureAlgorithm).with_source(err))?;

    // Note: Because we check for key type and algorithm compatiblity in generate/insert, these errors are impossible.
    let signature: Vec<u8> = match alg {
      JwsAlgorithm::EdDSA => {
        let okp_params = jwk.try_okp_params().map_err(|err| {
          KeyStorageError::new(KeyStorageErrorKind::Unspecified)
            .with_custom_message(format!("expected a Jwk with Okp params in order to sign with {alg}"))
            .with_source(err)
        })?;
        if okp_params.crv != EdCurve::Ed25519.name() {
          return Err(
            KeyStorageError::new(KeyStorageErrorKind::Unspecified).with_custom_message(format!(
              "expected Jwk with Okp {} crv in order to sign with {alg}",
              EdCurve::Ed25519
            )),
          );
        }

        let secret_key: _ = ed25519::expand_secret_jwk(jwk)?;
        secret_key.sign(&data).to_bytes().to_vec()
      }
      other => {
        return Err(
          KeyStorageError::new(KeyStorageErrorKind::UnsupportedSignatureAlgorithm)
            .with_custom_message(format!("{other} is not supported")),
        );
      }
    };

    Ok(signature)
  }

  async fn public(&self, key_id: &KeyId) -> KeyStorageResult<Jwk> {
    let jwk_store: RwLockReadGuard<'_, JwkKeyStore> = self.jwk_store.read().await;
    let jwk: &Jwk = jwk_store
      .get(key_id)
      .ok_or_else(|| KeyStorageError::new(KeyStorageErrorKind::KeyNotFound))?;
    Ok(jwk.to_public())
  }

  async fn delete(&self, key_id: &KeyId) -> KeyStorageResult<()> {
    let mut jwk_store: RwLockWriteGuard<'_, JwkKeyStore> = self.jwk_store.write().await;

    jwk_store
      .remove(key_id)
      .map(|_| ())
      .ok_or_else(|| KeyStorageError::new(KeyStorageErrorKind::KeyNotFound))
  }

  async fn exists(&self, key_id: &KeyId) -> KeyStorageResult<bool> {
    let jwk_store: RwLockReadGuard<'_, JwkKeyStore> = self.jwk_store.read().await;
    Ok(jwk_store.contains_key(key_id))
  }
}

pub(crate) mod ed25519 {
  use crypto::signatures::ed25519::PublicKey;
  use crypto::signatures::ed25519::SecretKey;
  use crypto::signatures::ed25519::{self};
  use identity_jose::jwk::EdCurve;
  use identity_jose::jwk::Jwk;
  use identity_jose::jwk::JwkParamsOkp;
  use identity_jose::jwu;

  use crate::key_storage::KeyStorageError;
  use crate::key_storage::KeyStorageErrorKind;
  use crate::key_storage::KeyStorageResult;

  pub(crate) fn expand_secret_jwk(jwk: &Jwk) -> KeyStorageResult<SecretKey> {
    let params: &JwkParamsOkp = jwk.try_okp_params().unwrap();

    if params
      .try_ed_curve()
      .map_err(|err| KeyStorageError::new(KeyStorageErrorKind::UnsupportedKeyType).with_source(err))?
      != EdCurve::Ed25519
    {
      return Err(
        KeyStorageError::new(KeyStorageErrorKind::UnsupportedKeyType)
          .with_custom_message(format!("expected an {} key", EdCurve::Ed25519.name())),
      );
    }

    let sk: [u8; ed25519::SECRET_KEY_LENGTH] = params
      .d
      .as_deref()
      .map(jwu::decode_b64)
      .ok_or_else(|| {
        KeyStorageError::new(KeyStorageErrorKind::Unspecified)
          .with_custom_message("expected Jwk `d` param to be present")
      })?
      .map_err(|err| {
        KeyStorageError::new(KeyStorageErrorKind::Unspecified)
          .with_custom_message("unable to decode `d` param")
          .with_source(err)
      })?
      .try_into()
      .map_err(|_| {
        KeyStorageError::new(KeyStorageErrorKind::Unspecified)
          .with_custom_message(format!("expected key of length {}", ed25519::SECRET_KEY_LENGTH))
      })?;

    Ok(SecretKey::from_bytes(sk))
  }

  pub(crate) fn encode_jwk(private_key: &SecretKey, public_key: &PublicKey) -> Jwk {
    let x = jwu::encode_b64(public_key.as_ref());
    let d = jwu::encode_b64(private_key.to_bytes().as_ref());
    let mut params = JwkParamsOkp::new();
    params.x = x;
    params.d = Some(d);
    params.crv = EdCurve::Ed25519.name().to_owned();
    Jwk::from_params(params)
  }
}

const ED25519_KEY_TYPE_STR: &str = "Ed25519";
pub const ED25519_KEY_TYPE: KeyType = KeyType::from_static_str(ED25519_KEY_TYPE_STR);

#[derive(Debug, Copy, Clone)]
enum MemStoreKeyType {
  Ed25519,
}

impl MemStoreKeyType {
  pub const fn name(&self) -> &'static str {
    match self {
      MemStoreKeyType::Ed25519 => "Ed25519",
    }
  }
}

impl Display for MemStoreKeyType {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(self.name())
  }
}

impl TryFrom<&KeyType> for MemStoreKeyType {
  type Error = KeyStorageError;

  fn try_from(value: &KeyType) -> Result<Self, Self::Error> {
    match value.as_str() {
      ED25519_KEY_TYPE_STR => Ok(MemStoreKeyType::Ed25519),
      _ => Err(KeyStorageError::new(KeyStorageErrorKind::UnsupportedKeyType)),
    }
  }
}

impl TryFrom<&Jwk> for MemStoreKeyType {
  type Error = KeyStorageError;

  fn try_from(jwk: &Jwk) -> Result<Self, Self::Error> {
    match jwk.kty() {
      JwkType::Okp => {
        let okp_params = jwk.try_okp_params().map_err(|err| {
          KeyStorageError::new(KeyStorageErrorKind::UnsupportedKeyType)
            .with_custom_message("expected Okp parameters for a JWK with `kty` Okp")
            .with_source(err)
        })?;
        match okp_params.try_ed_curve().map_err(|err| {
          KeyStorageError::new(KeyStorageErrorKind::UnsupportedKeyType)
            .with_custom_message("only Ed curves are supported for signing")
            .with_source(err)
        })? {
          EdCurve::Ed25519 => Ok(MemStoreKeyType::Ed25519),
          curve => Err(
            KeyStorageError::new(KeyStorageErrorKind::UnsupportedKeyType)
              .with_custom_message(format!("{curve} not supported")),
          ),
        }
      }
      other => Err(
        KeyStorageError::new(KeyStorageErrorKind::UnsupportedKeyType)
          .with_custom_message(format!("Jwk `kty` {other} not supported")),
      ),
    }
  }
}

impl Default for JwkMemStore {
  fn default() -> Self {
    Self::new()
  }
}

/// Generate a random alphanumeric string of len 32.
fn random_key_id() -> KeyId {
  KeyId::new(rand::distributions::Alphanumeric.sample_string(&mut rand::thread_rng(), 32))
}

/// Check that the key type can be used with the algorithm.
fn check_key_alg_compatibility(key_type: MemStoreKeyType, alg: JwsAlgorithm) -> KeyStorageResult<()> {
  match (key_type, alg) {
    (MemStoreKeyType::Ed25519, JwsAlgorithm::EdDSA) => Ok(()),
    (key_type, alg) => {
      return Err(
        KeyStorageError::new(crate::key_storage::KeyStorageErrorKind::KeyAlgorithmMismatch)
          .with_custom_message(format!("`cannot use key type `{key_type}` with algorithm `{alg}`")),
      )
    }
  }
}

pub(crate) mod shared {
  use core::fmt::Debug;
  use core::fmt::Formatter;
  use tokio::sync::RwLock;
  use tokio::sync::RwLockReadGuard;
  use tokio::sync::RwLockWriteGuard;

  #[derive(Default)]
  pub struct Shared<T>(RwLock<T>);

  impl<T> Shared<T> {
    pub fn new(data: T) -> Self {
      Self(RwLock::new(data))
    }

    pub async fn read(&self) -> RwLockReadGuard<'_, T> {
      self.0.read().await
    }

    pub async fn write(&self) -> RwLockWriteGuard<'_, T> {
      self.0.write().await
    }
  }

  impl<T: Debug> Debug for Shared<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
      Debug::fmt(&self.0, f)
    }
  }
}

#[cfg(test)]
mod tests {
  use crypto::signatures::ed25519::PublicKey;
  use crypto::signatures::ed25519::Signature;
  use crypto::signatures::ed25519::{self};
  use identity_jose::jwk::EcCurve;
  use identity_jose::jwk::JwkParamsEc;
  use identity_jose::jwk::JwkParamsOkp;
  use identity_jose::jwu;

  use super::*;

  #[tokio::test]
  async fn generate_and_sign() {
    let test_msg: &[u8] = b"test";
    let store: JwkMemStore = JwkMemStore::new();

    let JwkGenOutput { key_id, jwk } = store.generate(ED25519_KEY_TYPE, JwsAlgorithm::EdDSA).await.unwrap();

    let signature = store.sign(&key_id, test_msg.to_vec()).await.unwrap();

    let public_key: PublicKey = expand_public_jwk(&jwk);
    let signature: Signature = Signature::from_bytes(signature.try_into().unwrap());

    assert!(public_key.verify(&signature, test_msg));
    assert!(store.exists(&key_id).await.unwrap());
    store.delete(&key_id).await.unwrap();
  }

  #[tokio::test]
  async fn insert() {
    let store: JwkMemStore = JwkMemStore::new();

    let (private_key, public_key) = generate_ed25519();
    let mut jwk: Jwk = crate::key_storage::ed25519::encode_jwk(&private_key, &public_key);

    // INVALID: Inserting a Jwk without an `alg` parameter should fail.
    let err = store.insert(jwk.clone()).await.unwrap_err();
    assert!(matches!(err.kind(), KeyStorageErrorKind::UnsupportedSignatureAlgorithm));

    // VALID: Inserting a Jwk with all private key components set should succeed.
    jwk.set_alg(JwsAlgorithm::EdDSA.name());
    store.insert(jwk.clone()).await.unwrap();

    // INVALID: Inserting a Jwk with all private key components unset should fail.
    let err = store.insert(jwk.to_public()).await.unwrap_err();
    assert!(matches!(err.kind(), KeyStorageErrorKind::Unspecified))
  }

  #[tokio::test]
  async fn exists() {
    let store: JwkMemStore = JwkMemStore::new();
    assert!(!store.exists(&KeyId::new("non-existent-id")).await.unwrap());
  }

  #[tokio::test]
  async fn incompatible_key_type() {
    let store: JwkMemStore = JwkMemStore::new();

    let mut ec_params = JwkParamsEc::new();
    ec_params.crv = EcCurve::P256.name().to_owned();
    ec_params.x = "".to_owned();
    ec_params.y = "".to_owned();
    ec_params.d = Some("".to_owned());
    let jwk_ec = Jwk::from_params(ec_params);

    let err: _ = store.insert(jwk_ec).await.unwrap_err();
    assert!(matches!(err.kind(), KeyStorageErrorKind::UnsupportedKeyType));
  }

  #[tokio::test]
  async fn incompatible_key_alg() {
    let store: JwkMemStore = JwkMemStore::new();

    let (private_key, public_key) = generate_ed25519();
    let mut jwk: Jwk = crate::key_storage::ed25519::encode_jwk(&private_key, &public_key);
    jwk.set_alg(JwsAlgorithm::ES256.name());

    // INVALID: Inserting an Ed25519 key with the ES256 alg is not compatible.
    let err = store.insert(jwk.clone()).await.unwrap_err();
    assert!(matches!(err.kind(), KeyStorageErrorKind::KeyAlgorithmMismatch));
  }

  pub(crate) fn expand_public_jwk(jwk: &Jwk) -> PublicKey {
    let params: &JwkParamsOkp = jwk.try_okp_params().unwrap();

    if params.try_ed_curve().unwrap() != EdCurve::Ed25519 {
      panic!("expected an ed25519 jwk");
    }

    let pk: [u8; ed25519::PUBLIC_KEY_LENGTH] = jwu::decode_b64(params.x.as_str()).unwrap().try_into().unwrap();

    PublicKey::try_from(pk).unwrap()
  }

  fn generate_ed25519() -> (SecretKey, PublicKey) {
    let private_key = SecretKey::generate().unwrap();
    let public_key = private_key.public_key();
    (private_key, public_key)
  }
}
