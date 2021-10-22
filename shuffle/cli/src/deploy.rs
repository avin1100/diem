// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::shared::{build_move_packages, get_home_path, read_config, Home};
use anyhow::{anyhow, Result};
use diem_crypto::PrivateKey;
use diem_sdk::{
    transaction_builder::TransactionFactory,
    types::{
        transaction::{Module, TransactionPayload},
        LocalAccount,
    },
};
use diem_types::{chain_id::ChainId, transaction::authenticator::AuthenticationKey};
use generate_key::load_key;
use move_package::compilation::compiled_package::CompiledPackage;
use serde_json::{value::Value::Null, Value};
use std::path::Path;
use url::Url;

/// Deploys shuffle's main Move Package to the sender's address.
pub fn handle(project_path: &Path) -> Result<()> {
    let config = read_config(project_path)?;
    let home = Home::new(get_home_path().as_path())?;

    let account_key_path = home.get_latest_key_path();
    if !account_key_path.exists() {
        return Err(anyhow!(
            "An account hasn't been created yet! Run shuffle account first."
        ));
    }
    let compiled_package = build_move_packages(project_path)?;
    publish_packages_as_transaction(account_key_path, compiled_package, config.get_network())
}

fn publish_packages_as_transaction(
    account_key_path: &Path,
    compiled_package: CompiledPackage,
    network: &str,
) -> Result<()> {
    let new_account_key = load_key(account_key_path);
    let factory = TransactionFactory::new(ChainId::test());
    println!("Using Public Key {}", &new_account_key.public_key());
    let derived_address =
        AuthenticationKey::ed25519(&new_account_key.public_key()).derived_address();
    println!(
        "Sending txn from address {}",
        derived_address.to_hex_literal()
    );

    let path = Url::parse(
        format!(
            "http://{}/accounts/{}/resources",
            network,
            derived_address.to_hex_literal()
        )
        .as_str(),
    )?;
    let resp = ureq::get(path.as_str()).call()?.into_string()?;
    let json_objects: Vec<Value> = serde_json::from_str(&resp)?;

    let mut seq_number_string = "";
    for object in &json_objects {
        if object["type"]["name"] == "DiemAccount" {
            seq_number_string = object["value"]["sequence_number"].as_str().unwrap();
            break;
        };
    }
    let seq_number: u64 = seq_number_string.parse()?;
    let mut new_account = LocalAccount::new(derived_address, new_account_key, seq_number);
    let all_hashes =
        send_module_transaction(&compiled_package, &mut new_account, &factory, network)?;

    for hash in &all_hashes {
        confirm_txn_executed(hash.as_str(), network)?
    }

    Ok(())
}

pub fn send_module_transaction(
    compiled_package: &CompiledPackage,
    account: &mut LocalAccount,
    factory: &TransactionFactory,
    network: &str,
) -> Result<Vec<String>> {
    let mut vec = Vec::new();
    for module in compiled_package
        .transitive_compiled_modules()
        .compute_dependency_graph()
        .compute_topological_order()?
    {
        let module_id = module.self_id();
        if module_id.address() == &account.address() {
            println!("Deploying Module: {}", module_id);
            let mut binary = vec![];
            module.serialize(&mut binary)?;
            let publish_txn = account.sign_with_transaction_builder(
                factory.payload(TransactionPayload::Module(Module::new(binary))),
            );

            let bytes = bcs::to_bytes(&publish_txn)?;
            let hash = send_txn(bytes, network)?;
            vec.push(hash);
        } else {
            println!("Skipping Module: {}", module_id);
        }
    }
    println!("Success!");
    Ok(vec)
}

pub fn send_txn(txn_bytes: Vec<u8>, network: &str) -> Result<String> {
    let path = Url::parse(format!("http://{}/transactions", network).as_str())?;
    let resp = ureq::post(path.as_str())
        .set("Content-Type", "application/vnd.bcs+signed_transaction")
        .send_bytes(&*txn_bytes)?;
    let json: serde_json::Value = resp.into_json()?;
    let hash = json["hash"].as_str().unwrap();
    Ok(hash.to_string())
}

pub fn confirm_txn_executed(hash: &str, network: &str) -> Result<()> {
    let path = Url::parse(format!("http://{}/transactions/{}", network, hash).as_str())?;
    let mut resp = ureq::get(path.as_str()).call()?;
    let mut json: serde_json::Value = resp.into_json()?;

    while json["success"] == Null {
        resp = ureq::get(path.as_str()).call()?;
        json = resp.into_json()?;
    }

    if json["success"] == false {
        println!(
            "Transaction with hash {} didn't execute successfully: ",
            hash
        );
        println!("{:#?}", json);
    } else {
        println!("Transaction with hash {} executed successfully", hash);
    }
    Ok(())
}
