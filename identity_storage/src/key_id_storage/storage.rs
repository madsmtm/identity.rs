// Copyright 2020-2023 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::key_storage::KeyId;
use async_trait::async_trait;

use super::key_id_storage_error::KeyIdStorageError;
use super::method_digest::MethodDigest;

pub type KeyIdStorageResult<T> = Result<T, KeyIdStorageError>;

/// Storing
#[cfg_attr(not(feature = "send-sync-storage"), async_trait(?Send))]
#[cfg_attr(feature = "send-sync-storage", async_trait)]
pub trait KeyIdStorage {
  /// Insert a [`KeyId`] into the [`KeyIdStorage`] under the given [`MethodDigest`].
  /// If an entry for `key` already exists in the storage an error must be returned
  /// immediately without altering the state the storage.
  async fn insert_key_id(&self, key: MethodDigest, value: KeyId) -> KeyIdStorageResult<()>;

  /// Obtain the [`KeyId`] associated with the given [`MethodDigest`].
  async fn get_key_id(&self, key: &MethodDigest) -> KeyIdStorageResult<KeyId>;

  /// Delete the [`KeyId`] associated with the given [`MethodDigest`] from the [`IdentityStorage`].
  /// If `key` is not found in storage, an Error must be returned.
  async fn delete_key_id(&self, key: &MethodDigest) -> KeyIdStorageResult<()>;
}