// Copyright 2020-2021 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use crate::{types::RequestContext, Actor, IdentityList};
use identity_account::{account::Account, identity::IdentityCreate};
use identity_iota::{
  did::{IotaDID, IotaDocument},
  tangle::{ClientBuilder, ClientMap, Network, TangleResolve},
};

use super::{requests::IdentityResolve, StorageError};

#[derive(Clone)]
pub struct StorageHandler {
  client: Arc<ClientMap>,
}

impl StorageHandler {
  pub async fn new() -> identity_account::Result<Self> {
    let builder = ClientBuilder::new().network(Network::Mainnet);

    Ok(Self {
      client: Arc::new(ClientMap::from_builder(builder).await?),
    })
  }

  pub async fn create(
    self,
    _actor: Actor,
    request: RequestContext<IdentityCreate>,
  ) -> Result<IotaDocument, StorageError> {
    let acc = Account::builder().build().await.unwrap();
    let snapshot = acc.create_identity(request.input).await?;
    let doc = snapshot.to_document()?;
    Ok(doc)
  }

  pub async fn list(self, _actor: Actor, _request: RequestContext<IdentityList>) -> Vec<IotaDID> {
    vec![]
  }

  pub async fn resolve(
    self,
    _actor: Actor,
    request: RequestContext<IdentityResolve>,
  ) -> Result<IotaDocument, StorageError> {
    log::info!("Resolving {:?}", request.input.did);

    let res = self.client.resolve(&request.input.did).await?;

    log::info!("Resolved into: {:?}", res);

    Ok(res)
  }
}
