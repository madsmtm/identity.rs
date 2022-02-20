// Copyright 2020-2022 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::ActorRequest;
use crate::Endpoint;
use crate::Result;

use libp2p::PeerId;
use serde::Deserialize;
use serde::Serialize;
use std::fmt::Debug;
use std::fmt::Display;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMessage
// where
//   T: Serialize + for<'deser> Deserialize<'deser>,
{
  pub endpoint: Endpoint,
  pub data: Vec<u8>,
}

impl RequestMessage
// where
//   T: Serialize + for<'deser> Deserialize<'deser>,
{
  pub fn new(name: impl AsRef<str>, data: Vec<u8>) -> Result<Self> {
    Ok(Self {
      endpoint: Endpoint::new(name)?,
      data,
    })
  }

  pub fn from_bytes(bytes: &[u8]) -> std::io::Result<Self> {
    // TODO: Replace with some serialization that's faster or more compact?
    serde_json::from_slice::<'_, Self>(bytes)
      .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))
  }

  pub fn to_bytes(&self) -> std::io::Result<Vec<u8>> {
    serde_json::to_vec(self).map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))
  }
}

pub type ResponseMessage = Vec<u8>;

pub struct RequestContext<T> {
  pub input: T,
  pub peer: PeerId,
  pub endpoint: Endpoint,
}

impl<T> RequestContext<T> {
  pub fn new(input: T, peer: PeerId, endpoint: Endpoint) -> Self {
    Self { input, peer, endpoint }
  }

  pub fn convert<I>(self, input: I) -> RequestContext<I> {
    RequestContext::new(input, self.peer, self.endpoint)
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DidCommPlaintextMessage<T> {
  pub typ: String,
  pub id: ThreadId,
  pub thid: Option<ThreadId>,
  pub pthid: Option<ThreadId>,
  #[serde(rename = "type")]
  pub type_: String,
  pub from: String,
  pub to: String,
  pub created_time: u32,
  pub expires_time: u32,
  pub body: T,
}

impl<T> DidCommPlaintextMessage<T> {
  pub fn new(id: ThreadId, type_: String, body: T) -> Self {
    DidCommPlaintextMessage {
      id,
      type_,
      body,
      typ: String::new(),
      thid: None,
      pthid: None,
      from: String::new(),
      to: String::new(),
      created_time: 0,
      expires_time: 0,
    }
  }

  pub fn thread_id(&self) -> &ThreadId {
    match self.thid.as_ref() {
      Some(thid) => thid,
      None => &self.id,
    }
  }
}

impl<T> ActorRequest for DidCommPlaintextMessage<T>
where
  T: ActorRequest,
{
  type Response = ();

  fn request_name<'cow>(&self) -> std::borrow::Cow<'cow, str> {
    self.body.request_name()
  }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadId(Uuid);

impl ThreadId {
  #[allow(clippy::new_without_default)]
  pub fn new() -> Self {
    Self(Uuid::new_v4())
  }
}

impl Display for ThreadId {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.0)
  }
}

/// Can be returned from a hook to indicate that the protocol should immediately terminate.
/// This doesn't include any way to set a cause for the termination, as it is expected that
/// a hook sends a problem report to the peer before returning this type.
pub struct DidCommTermination;
