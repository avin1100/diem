// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::shared::{
    build_move_packages, get_home_path, normalized_network, read_config, DevApiClient, Home,
};
use anyhow::{anyhow, Result};
use diem_crypto::PrivateKey;
use diem_sdk::{
    client::AccountAddress,
    transaction_builder::TransactionFactory,
    types::{
        transaction::{Module, TransactionPayload},
        LocalAccount,
    },
};
use diem_types::{chain_id::ChainId, transaction::authenticator::AuthenticationKey};
use generate_key::load_key;
use serde_json::Value;
use std::{
    path::Path,
    time::{Duration, Instant},
};

/// Deploys shuffle's main Move Package to the sender's address.
pub async fn handle(project_path: &Path) -> Result<()> {
    let network = normalized_network(read_config(project_path)?.get_network())?;
    let client = DevApiClient::new(reqwest::Client::new(), network)?;

    let home = Home::new(get_home_path().as_path())?;
    if !home.get_latest_key_path().exists() {
        return Err(anyhow!(
            "An account hasn't been created yet! Run shuffle account first."
        ));
    }
    let new_account_key = load_key(home.get_latest_key_path());
    println!("Using Public Key {}", &new_account_key.public_key());
    let derived_address =
        AuthenticationKey::ed25519(&new_account_key.public_key()).derived_address();
    println!(
        "Sending txn from address {}",
        derived_address.to_hex_literal()
    );

    let account_seq_number = client.get_account_sequence_number(derived_address).await?;
    let mut new_account = LocalAccount::new(derived_address, new_account_key, account_seq_number);

    let mut all_module_names = Vec::new();
    let mut all_hashes = Vec::new();

    let compiled_package = build_move_packages(project_path)?;

    for module in compiled_package
        .transitive_compiled_modules()
        .compute_dependency_graph()
        .compute_topological_order()?
    {
        let module_id = module.self_id();
        if module_id.address() != &new_account.address() {
            println!("Skipping Module: {}", module_id);
            continue;
        }
        println!("Deploying Module: {}", module_id);
        let mut binary = vec![];
        module.serialize(&mut binary)?;

        let hash = send_module_transaction(&client, &mut new_account, binary).await?;

        all_module_names.push(module_id.name().as_str().to_string());
        all_hashes.push(hash);
    }

    confirm_txns_executed_from_hashes(&client, all_hashes).await?;
    confirm_modules_exist(&client, all_module_names, derived_address).await
}

async fn send_module_transaction(
    client: &DevApiClient,
    account: &mut LocalAccount,
    module_binary: Vec<u8>,
) -> Result<String> {
    let factory = TransactionFactory::new(ChainId::test());
    let publish_txn = account.sign_with_transaction_builder(
        factory.payload(TransactionPayload::Module(Module::new(module_binary))),
    );
    let bytes = bcs::to_bytes(&publish_txn)?;
    let resp = client.post_transactions(bytes).await?;
    let json: serde_json::Value = serde_json::from_str(resp.text().await?.as_str())?;
    let hash = get_hash_from_post_txn(json)?;
    Ok(hash)
}

pub async fn confirm_txns_executed_from_hashes(
    client: &DevApiClient,
    all_hashes: Vec<String>,
) -> Result<()> {
    for hash in all_hashes.iter() {
        let mut resp = client.get_transactions_by_hash(hash.as_str()).await?;
        let mut json: serde_json::Value = serde_json::from_str(resp.text().await?.as_str())?;
        let start = Instant::now();
        while json["type"] == "pending_transaction" {
            resp = client.get_transactions_by_hash(hash.as_str()).await?;
            json = serde_json::from_str(resp.text().await?.as_str())?;
            let duration = start.elapsed();
            if duration > Duration::from_secs(10) {
                break;
            }
        }
        if is_execution_successful(&json)? {
            println!("Transaction with hash {} executed successfully", hash);
            return Ok(());
        }
        println!(
            "Transaction with hash {} didn't execute successfully:",
            hash
        );
        println!("{:#?}", &json);
    }
    Ok(())
}

fn is_execution_successful(json: &Value) -> Result<bool> {
    json["success"]
        .as_bool()
        .ok_or_else(|| anyhow!("Unable to access success key"))
}

pub async fn confirm_modules_exist(
    client: &DevApiClient,
    all_names: Vec<String>,
    address: AccountAddress,
) -> Result<()> {
    let resp = client.get_account_modules(address).await?;
    let json: serde_json::Value = serde_json::from_str(resp.text().await?.as_str())?;
    let all_modules = json
        .as_array()
        .ok_or_else(|| anyhow!("Failed to get modules"))?;

    for module in all_modules.iter() {
        let module_name = module["abi"]["name"].to_string();
        if does_module_exist(module_name.as_str(), all_names.to_vec()) {
            println!("The {} module exists", module_name);
        } else {
            println!("The {} module doesn't exists", module_name);
        }
    }
    Ok(())
}

fn does_module_exist(module_name: &str, deployed_modules_names: Vec<String>) -> bool {
    deployed_modules_names.contains(&parse_module_name(module_name))
}

fn parse_module_name(module_name: &str) -> String {
    module_name.replace('"', "")
}

fn get_hash_from_post_txn(json: Value) -> Result<String> {
    Ok(json["hash"].as_str().unwrap().to_string())
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;

    fn post_txn_json_output() -> Value {
        json!({
        "type":"pending_transaction",
        "hash":"0xbca2738726dc456f23762372ab0dd2f450ec3ec20271e5318ae37e9d42ee2bb8",
        "sender":"0x24163afcc6e33b0a9473852e18327fa9",
        "sequence_number":"10",
        "max_gas_amount":"1000000",
        "gas_unit_price":"0",
        "gas_currency_code":"XUS",
        "expiration_timestamp_secs":"1635872777",
        "payload":{}
        })
    }

    fn get_transactions_by_hash_json_output_success() -> Value {
        json!({
            "type":"user_transaction",
            "version":"3997",
            "hash":"0x89e59bb50521334a69c06a315b6dd191a8da4c1c7a40ce27a8f96f12959496eb",
            "state_root_hash":"0x7a0b81379ab8786f34fcff804e5fb413255467c28f09672e8d22bfaa4e029102",
            "event_root_hash":"0x414343554d554c41544f525f504c414345484f4c4445525f4841534800000000",
            "gas_used":"8",
            "success":true,
            "vm_status":"Executed successfully",
            "sender":"0x24163afcc6e33b0a9473852e18327fa9",
            "sequence_number":"14",
            "max_gas_amount":"1000000",
            "gas_unit_price":"0",
            "gas_currency_code":"XUS",
            "expiration_timestamp_secs":"1635873470",
            "payload":{}
        })
    }

    fn get_transactions_by_hash_json_output_fail() -> Value {
        json!({
            "type":"user_transaction",
            "version":"3997",
            "hash":"0x89e59bb50521334a69c06a315b6dd191a8da4c1c7a40ce27a8f96f12959496eb",
            "state_root_hash":"0x7a0b81379ab8786f34fcff804e5fb413255467c28f09672e8d22bfaa4e029102",
            "event_root_hash":"0x414343554d554c41544f525f504c414345484f4c4445525f4841534800000000",
            "gas_used":"8",
            "success":false,
            "vm_status":"miscellaneous error",
            "sender":"0x24163afcc6e33b0a9473852e18327fa9",
            "sequence_number":"14",
            "max_gas_amount":"1000000",
            "gas_unit_price":"0",
            "gas_currency_code":"XUS",
            "expiration_timestamp_secs":"1635873470",
            "payload":{}
        })
    }

    #[test]
    fn test_confirm_is_execution_successful() {
        let successful_txn = get_transactions_by_hash_json_output_success();
        assert_eq!(is_execution_successful(&successful_txn).unwrap(), true);

        let failed_txn = get_transactions_by_hash_json_output_fail();
        assert_eq!(is_execution_successful(&failed_txn).unwrap(), false);
    }

    #[test]
    fn test_does_module_exist() {
        let module_names = vec![
            String::from("Message"),
            String::from("NFT"),
            String::from("TestNFT"),
        ];
        assert_eq!(does_module_exist("Message", module_names.to_vec()), true);
        assert_eq!(does_module_exist("NFT", module_names.to_vec()), true);
        assert_eq!(does_module_exist("TestNFT", module_names.to_vec()), true);
        assert_eq!(
            does_module_exist("Fake Module", module_names.to_vec()),
            false
        );
    }

    #[test]
    fn test_get_hash_from_post_txn() {
        let txn = post_txn_json_output();
        let hash = get_hash_from_post_txn(txn).unwrap();
        assert_eq!(
            hash,
            "0xbca2738726dc456f23762372ab0dd2f450ec3ec20271e5318ae37e9d42ee2bb8"
        );
    }

    #[test]
    fn test_parse_module_name() {
        let raw_module_name = r#""NFT""#;
        assert_eq!(raw_module_name == "NFT", false);
        let corrected_name = parse_module_name(raw_module_name);
        assert_eq!(corrected_name, "NFT");
    }
}
