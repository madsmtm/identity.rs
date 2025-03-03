// Copyright 2020-2022 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use examples::create_did;
use examples::random_stronghold_path;
use examples::API_ENDPOINT;
use identity_iota::crypto::KeyPair;
use identity_iota::iota::block::output::feature::MetadataFeature;
use identity_iota::iota::IotaDID;
use identity_iota::iota::IotaDocument;
use identity_iota::iota::IotaIdentityClientExt;
use identity_iota::iota::NetworkName;
use iota_client::block::address::Address;
use iota_client::block::address::AliasAddress;
use iota_client::block::output::dto::OutputDto;
use iota_client::block::output::feature::IssuerFeature;
use iota_client::block::output::unlock_condition::AddressUnlockCondition;
use iota_client::block::output::AliasId;
use iota_client::block::output::Feature;
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

/// Demonstrates how an identity can issue and own NFTs,
/// and how observers can verify the issuer of the NFT.
///
/// For this example, we consider the case where a manufacturer issues
/// a digital product passport (DPP) as an NFT.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
  // ==============================================
  // Create the manufacturer's DID and the DPP NFT.
  // ==============================================

  // Create a new client to interact with the IOTA ledger.
  let client: Client = Client::builder().with_primary_node(API_ENDPOINT, None)?.finish()?;

  // Create a new secret manager backed by a Stronghold.
  let mut secret_manager: SecretManager = SecretManager::Stronghold(
    StrongholdSecretManager::builder()
      .password("secure_password")
      .build(random_stronghold_path())?,
  );

  // Create a new DID for the manufacturer.
  let (_, manufacturer_document, _): (Address, IotaDocument, KeyPair) =
    create_did(&client, &mut secret_manager).await?;
  let manufacturer_did = manufacturer_document.id().clone();

  // Get the current byte cost.
  let rent_structure: RentStructure = client.get_rent_structure().await?;

  // Create a Digital Product Passport NFT issued by the manufacturer.
  let product_passport_nft: NftOutput =
    NftOutputBuilder::new_with_minimum_storage_deposit(rent_structure, NftId::null())?
      // The NFT will initially be owned by the manufacturer.
      .add_unlock_condition(UnlockCondition::Address(AddressUnlockCondition::new(Address::Alias(
        AliasAddress::new(AliasId::from(&manufacturer_did)),
      ))))
      // Set the manufacturer as the immutable issuer.
      .add_immutable_feature(Feature::Issuer(IssuerFeature::new(Address::Alias(AliasAddress::new(
        AliasId::from(&manufacturer_did),
      )))))
      // A proper DPP would hold its metadata here.
      .add_immutable_feature(Feature::Metadata(MetadataFeature::new(
        b"Digital Product Passport Metadata".to_vec(),
      )?))
      .finish(client.get_token_supply().await?)?;

  // Publish the NFT.
  let block: Block = client
    .block()
    .with_secret_manager(&secret_manager)
    .with_outputs(vec![product_passport_nft.into()])?
    .finish()
    .await?;
  let _ = client.retry_until_included(&block.id(), None, None).await?;

  // ========================================================
  // Resolve the Digital Product Passport NFT and its issuer.
  // ========================================================

  // Extract the identifier of the NFT from the published block.
  let nft_id: NftId = NftId::from(&get_nft_output_id(
    block
      .payload()
      .ok_or_else(|| anyhow::anyhow!("expected block to contain a payload"))?,
  )?);

  // Fetch the NFT Output.
  let nft_output_id: OutputId = client.nft_output_id(nft_id).await?;
  let output_dto: OutputDto = client
    .get_output(&nft_output_id)
    .await
    .map(|response| response.output)?;
  let output: Output = Output::try_from_dto(&output_dto, client.get_token_supply().await?)?;

  // Extract the issuer of the NFT.
  let nft_output: NftOutput = if let Output::Nft(nft_output) = output {
    nft_output
  } else {
    anyhow::bail!("expected NFT output")
  };

  let issuer_address: Address = if let Some(Feature::Issuer(issuer)) = nft_output.immutable_features().iter().next() {
    *issuer.address()
  } else {
    anyhow::bail!("expected an issuer feature")
  };

  let manufacturer_alias_id: AliasId = if let Address::Alias(alias_address) = issuer_address {
    *alias_address.alias_id()
  } else {
    anyhow::bail!("expected an Alias Address")
  };

  // Reconstruct the manufacturer's DID from the Alias Id.
  let network: NetworkName = client.network_name().await?;
  let manufacturer_did: IotaDID = IotaDID::new(&manufacturer_alias_id, &network);

  // Resolve the issuer of the NFT.
  let manufacturer_document: IotaDocument = client.resolve_did(&manufacturer_did).await?;

  println!("The issuer of the Digital Product Passport NFT is: {manufacturer_document:#}");

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
