// Copyright 2020-2022 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use libp2p::Multiaddr;

use crate::Actor;
use crate::ActorBuilder;
use crate::ActorRequest;
use crate::Error;
use crate::IdentityResolve;
use crate::RequestContext;
use crate::Synchronous;

use super::default_listening_actor;
use super::default_sending_actor;

#[tokio::test]
async fn test_unknown_request_returns_error() -> crate::Result<()> {
  let (listening_actor, addr, peer_id) = default_listening_actor().await;

  let mut sending_actor = default_sending_actor().await;
  sending_actor.add_address(peer_id, addr).await;

  let result = sending_actor
    .send_named_request(
      peer_id,
      "unknown/request",
      IdentityResolve::new("did:iota:FFFAH6qct9KGQcSenG1iaw2Nj9jP7Zmug2zcmTpF4942".parse().unwrap()),
    )
    .await;

  assert!(matches!(result.unwrap_err(), Error::UnknownRequest(_)));

  listening_actor.shutdown().await.unwrap();
  sending_actor.shutdown().await.unwrap();

  Ok(())
}

#[tokio::test]
async fn test_actors_can_communicate_bidirectionally() -> crate::Result<()> {
  let mut actor1 = ActorBuilder::new().build().await.unwrap();
  let mut actor2 = ActorBuilder::new().build().await.unwrap();

  actor2
    .start_listening("/ip4/0.0.0.0/tcp/0".parse().unwrap())
    .await
    .unwrap();

  let addr: Multiaddr = actor2.addresses().await.into_iter().next().unwrap();

  #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
  pub struct Dummy(u8);

  impl ActorRequest<Synchronous> for Dummy {
    type Response = ();

    fn request_name<'cow>(&self) -> std::borrow::Cow<'cow, str> {
      std::borrow::Cow::Borrowed("request/test")
    }
  }

  #[derive(Clone)]
  pub struct State(pub Arc<AtomicBool>);

  impl State {
    async fn handler(self, _actor: Actor, _req: RequestContext<Dummy>) {
      self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
  }

  let actor1_state = State(Arc::new(AtomicBool::new(false)));
  let actor2_state = State(Arc::new(AtomicBool::new(false)));

  actor1
    .add_state(actor1_state.clone())
    .add_handler("request/test", State::handler)
    .unwrap();

  actor2
    .add_state(actor2_state.clone())
    .add_handler("request/test", State::handler)
    .unwrap();

  actor1.add_address(actor2.peer_id(), addr).await;

  actor1.send_request(actor2.peer_id(), Dummy(42)).await.unwrap();

  actor2.send_request(actor1.peer_id(), Dummy(43)).await.unwrap();

  actor1.shutdown().await.unwrap();
  actor2.shutdown().await.unwrap();

  assert!(actor1_state.0.load(std::sync::atomic::Ordering::SeqCst));
  assert!(actor2_state.0.load(std::sync::atomic::Ordering::SeqCst));

  Ok(())
}

#[tokio::test]
async fn test_actor_handler_is_invoked() -> crate::Result<()> {
  let (mut receiver, receiver_addr, receiver_peer_id) = default_listening_actor().await;
  let mut sender = default_sending_actor().await;

  #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
  pub struct Dummy(u8);

  impl ActorRequest<Synchronous> for Dummy {
    type Response = ();

    fn request_name<'cow>(&self) -> std::borrow::Cow<'cow, str> {
      std::borrow::Cow::Borrowed("request/test")
    }
  }

  #[derive(Clone)]
  pub struct State(pub Arc<AtomicBool>);

  impl State {
    async fn handler(self, _actor: Actor, req: RequestContext<Dummy>) {
      if let Dummy(42) = req.input {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
      }
    }
  }

  let state = State(Arc::new(AtomicBool::new(false)));

  receiver
    .add_state(state.clone())
    .add_handler("request/test", State::handler)
    .unwrap();

  sender.add_address(receiver_peer_id, receiver_addr).await;

  sender.send_request(receiver_peer_id, Dummy(42)).await.unwrap();

  sender.shutdown().await.unwrap();
  receiver.shutdown().await.unwrap();

  assert!(state.0.load(std::sync::atomic::Ordering::SeqCst));

  Ok(())
}

#[tokio::test]
async fn test_synchronous_handler_invocation() -> crate::Result<()> {
  pretty_env_logger::init();

  let (mut listening_actor, addr, peer_id) = default_listening_actor().await;

  let mut sending_actor = default_sending_actor().await;
  sending_actor.add_address(peer_id, addr).await;

  #[derive(Debug, serde::Serialize, serde::Deserialize)]
  pub struct MessageResponse(String);

  #[derive(Debug, serde::Serialize, serde::Deserialize)]
  pub struct MessageRequest(String);

  impl ActorRequest<Synchronous> for MessageRequest {
    type Response = MessageResponse;

    fn request_name<'cow>(&self) -> std::borrow::Cow<'cow, str> {
      std::borrow::Cow::Borrowed("test/message")
    }
  }

  listening_actor
    .add_state(())
    .add_handler(
      "test/message",
      |_: (), _: Actor, message: RequestContext<MessageRequest>| async move {
        println!("invoked");
        MessageResponse(message.input.0)
      },
    )
    .unwrap();

  let result = sending_actor
    .send_request(peer_id, MessageRequest("test".to_owned()))
    .await;

  assert_eq!(result.unwrap().0, "test".to_owned());

  listening_actor.shutdown().await.unwrap();
  sending_actor.shutdown().await.unwrap();

  Ok(())
}

#[should_panic]
#[tokio::test]
async fn test_interacting_with_shutdown_actor_panics() {
  let (listening_actor, _, _) = default_listening_actor().await;

  let mut commander_copy = listening_actor.commander.clone();

  listening_actor.shutdown().await.unwrap();

  commander_copy.get_addresses().await;
}
