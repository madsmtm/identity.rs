// Copyright 2020-2022 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use examples::create_did_document;
use examples::get_address_with_funds;
use examples::random_stronghold_path;
use examples::API_ENDPOINT;
use examples::FAUCET_ENDPOINT;
use identity_iota::iota::block::address::NftAddress;
use identity_iota::iota::block::output::AliasOutput;
use identity_iota::iota::IotaClientExt;
use identity_iota::iota::IotaDocument;
use identity_iota::iota::IotaIdentityClientExt;
use identity_iota::iota::NetworkName;
use iota_client::block::address::Address;
use iota_client::block::output::dto::OutputDto;
use iota_client::block::output::unlock_condition::AddressUnlockCondition;
use iota_client::block::output::NftId;
use iota_client::block::output::NftOutput;
use iota_client::block::output::NftOutputBuilder;
use iota_client::block::output::Output;
use iota_client::block::output::OutputId;
use iota_client::block::output::RentStructure;
use iota_client::block::output::UnlockCondition;
use iota_client::block::payload::transaction::TransactionEssence;
use iota_client::block::payload::Payload;
use iota_client::block::Block;
use iota_client::secret::stronghold::StrongholdSecretManager;
use iota_client::secret::SecretManager;
use iota_client::Client;

/// Demonstrates how an identity can be owned by NFTs,
/// and how observers can verify that relationship.
///
/// For this example, we consider the case where a car's NFT owns
/// the DID of the car, so that transferring the NFT also transfers DID ownership.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
  // =============================
  // Create the car's NFT and DID.
  // =============================

  // Create a new client to interact with the IOTA ledger.
  let client: Client = Client::builder().with_primary_node(API_ENDPOINT, None)?.finish()?;

  // Create a new secret manager backed by a Stronghold.
  let mut secret_manager: SecretManager = SecretManager::Stronghold(
    StrongholdSecretManager::builder()
      .password("secure_password")
      .build(random_stronghold_path())?,
  );

  // Get an address with funds for testing.
  let address: Address = get_address_with_funds(&client, &mut secret_manager, FAUCET_ENDPOINT).await?;

  // Get the current byte cost.
  let rent_structure: RentStructure = client.get_rent_structure().await?;

  // Create the car NFT with an Ed25519 address as the unlock condition.
  let car_nft: NftOutput = NftOutputBuilder::new_with_minimum_storage_deposit(rent_structure.clone(), NftId::null())?
    .add_unlock_condition(UnlockCondition::Address(AddressUnlockCondition::new(address)))
    .finish(client.get_token_supply().await?)?;

  // Publish the NFT output.
  let block: Block = client
    .block()
    .with_secret_manager(&secret_manager)
    .with_outputs(vec![car_nft.into()])?
    .finish()
    .await?;
  let _ = client.retry_until_included(&block.id(), None, None).await?;

  let car_nft_id: NftId = NftId::from(&get_nft_output_id(
    block
      .payload()
      .ok_or_else(|| anyhow::anyhow!("expected the block to contain a payload"))?,
  )?);

  let network: NetworkName = client.network_name().await?;

  // Construct a DID document for the car.
  let (car_document, _): (IotaDocument, _) = create_did_document(&network)?;

  // Create a new DID for the car that is owned by the car NFT.
  let car_did_output: AliasOutput = client
    .new_did_output(Address::Nft(car_nft_id.into()), car_document, Some(rent_structure))
    .await?;

  // Publish the car DID.
  let car_document: IotaDocument = client.publish_did_output(&secret_manager, car_did_output).await?;

  // ============================================
  // Determine the car's NFT given the car's DID.
  // ============================================

  // Resolve the Alias Output of the DID.
  let output: AliasOutput = client.resolve_did_output(car_document.id()).await?;

  // Extract the NFT address from the state controller unlock condition.
  let unlock_condition: &UnlockCondition = output
    .unlock_conditions()
    .iter()
    .next()
    .ok_or_else(|| anyhow::anyhow!("expected at least one unlock condition"))?;

  let car_nft_address: NftAddress =
    if let UnlockCondition::StateControllerAddress(state_controller_unlock_condition) = unlock_condition {
      if let Address::Nft(nft_address) = state_controller_unlock_condition.address() {
        *nft_address
      } else {
        anyhow::bail!("expected an NFT address as the unlock condition");
      }
    } else {
      anyhow::bail!("expected an Address as the unlock condition");
    };

  // Retrieve the NFT Output of the car.
  let car_nft_id: &NftId = car_nft_address.nft_id();
  let output_id: OutputId = client.nft_output_id(*car_nft_id).await?;
  let output_dto: OutputDto = client.get_output(&output_id).await.map(|response| response.output)?;
  let output: Output = Output::try_from_dto(&output_dto, client.get_token_supply().await?)?;

  let car_nft: NftOutput = if let Output::Nft(nft_output) = output {
    nft_output
  } else {
    anyhow::bail!("expected an NFT output");
  };

  println!("The car's DID is: {car_document:#}");
  println!("The car's NFT is: {car_nft:#?}");

  Ok(())
}

// Helper function to get the output id for the first NFT output in a Block.
fn get_nft_output_id(payload: &Payload) -> anyhow::Result<OutputId> {
  match payload {
    Payload::Transaction(tx_payload) => {
      let TransactionEssence::Regular(regular) = tx_payload.essence();
      for (index, output) in regular.outputs().iter().enumerate() {
        if let Output::Nft(_nft_output) = output {
          return Ok(OutputId::new(tx_payload.id(), index.try_into().unwrap())?);
        }
      }
      anyhow::bail!("no NFT output in transaction essence")
    }
    _ => anyhow::bail!("No transaction payload"),
  }
}
