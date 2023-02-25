// Copyright 2020-2023 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use crate::jwk::Jwk;
use crate::jwt::JwtHeaderSet;

use super::JwsAlgorithm;
use super::JwsHeader;

pub type JwsUnprotectedHeader<'a> = &'a JwsHeader;

pub type HeaderSet<'a> = JwtHeaderSet<'a, JwsHeader>;

pub type JWSVerificationAlgFnPtr =
  fn(&Jwk, VerificationInput<'_>) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>>;

pub struct JWSValidationConfig {
  /// A list of permitted extension parameters.
  crits: Option<Vec<String>>,
  /// Require that the `alg` parameter of the given `JWK` is set (and corresponds to the value
  /// extracted from the `header`).
  jwk_must_have_alg: bool,
}

impl Default for JWSValidationConfig {
  fn default() -> Self {
    Self {
      crits: None,
      jwk_must_have_alg: true,
    }
  }
}

#[derive(Default)]
pub struct JWSVerifier {
  /// A map of algorithm specific handlers.
  commands: HashMap<JwsAlgorithm, JWSVerificationAlgFnPtr>,
  /// The configuration providing application dependent criteria.
  config: JWSValidationConfig,
}

/// Input intended for an `alg` specific
/// JWS verifier.
pub struct VerificationInput<'a> {
  jose_header: HeaderSet<'a>,
  signing_input: Box<[u8]>,
  signature: &'a [u8],
}

impl<'a> VerificationInput<'a> {
  /// Decoded JOSE header.
  pub fn jose_header(&self) -> &HeaderSet<'a> {
    &self.jose_header
  }

  /// Signing input.
  pub fn signing_input(&self) -> &[u8] {
    self.signing_input.as_ref()
  }

  /// Decoded signature.
  pub fn signature(&self) -> &'a [u8] {
    self.signature
  }
}

impl JWSVerifier {
  pub fn new() -> Self {
    Self::default()
  }
  // TODO: Better error.
  pub fn attach_handler(&mut self, alg: JwsAlgorithm, handler: JWSVerificationAlgFnPtr) -> Result<(), String> {
    let Entry::Vacant(entry) = self.commands.entry(alg) else {
            return Err("a handler already exists for the given algorithm".into());
        };
    entry.insert(handler);
    Ok(())
  }

  pub fn validate_with<F, E>(
    decoded_parameters: VerificationInput,
    public_key: Option<&Jwk>,
    verification_algorithm_handler: F,
    config: &JWSValidationConfig,
  ) -> Result<(), crate::error::Error>
  where
    F: FnOnce(&VerificationInput, &Jwk) -> Result<(), E>,
    E: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
  {
    // Validate the header(s).
    let jose_header = &decoded_parameters.jose_header;
    crate::jwu::validate_jws_headers(
      jose_header.protected(),
      jose_header.unprotected(),
      config.crits.as_deref(),
    )?;

    // Obtain a JWK before proceeding. If `public_key` is `None`, we attempt to extract the JWK from the header.
    // If `public_key` is `Some` and we also find a JWK in the header we check that they are the same JWK.
    let key: &Jwk = {
      match (public_key, jose_header.jwk()) {
        (Some(key), None) => key,
        (None, Some(key)) => key,
        (Some(given_key), Some(extracted_key)) => Some(given_key).filter(|this| this == &extracted_key).ok_or(
          crate::error::Error::SignatureVerificationError(
            "mismatch between the header's jwk and the one proved".into(),
          ),
        )?,
        (None, None) => {
          return Err(crate::error::Error::SignatureVerificationError(
            "no public key could be extracted".into(),
          ));
        }
      }
    };

    let alg = jose_header.try_alg()?;

    // Validate the header's alg against the requirements of the JWK.
    {
      if let Some(key_alg) = key.alg() {
        if alg.name() != key_alg {
          return Err(crate::error::Error::SignatureVerificationError(
            "algorithm mismatch between jwk and jws header".into(),
          ));
        }
      } else if config.jwk_must_have_alg {
        return Err(crate::error::Error::SignatureVerificationError(
          "the jwk is missing the alg parameter required by the given config".into(),
        ));
      }
    }

    verification_algorithm_handler(&decoded_parameters, key)
      .map_err(|err| crate::error::Error::SignatureVerificationError(err.into()))
  }
}
