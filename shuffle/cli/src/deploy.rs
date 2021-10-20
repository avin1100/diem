// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::shared::{build_move_packages, get_shuffle_dir, read_config};
use anyhow::{anyhow, Result};
use diem_crypto::PrivateKey;
use diem_sdk::{
    client::BlockingClient,
    transaction_builder::TransactionFactory,
    types::{
        transaction::{Module, TransactionPayload},
        LocalAccount,
    },
};
use diem_types::{
    chain_id::ChainId,
    transaction::authenticator::AuthenticationKey,
};
use generate_key::load_key;
use move_package::compilation::compiled_package::CompiledPackage;
use std::{path::Path};

/// Deploys shuffle's main Move Package to the sender's address.
pub fn handle(project_path: &Path) -> Result<()> {
    let config = read_config(project_path)?;

    let account_key_path = get_shuffle_dir()
        .join("accounts")
        .join("latest")
        .join("dev.key");
    if !account_key_path.exists() {
        return Err(anyhow!(
            "An account hasn't been created yet! Run shuffle account first."
        ));
    }
    let compiled_package = build_move_packages(project_path)?;
    publish_packages_as_transaction(&account_key_path, compiled_package, config.get_network())
}

fn publish_packages_as_transaction(
    account_key_path: &Path,
    compiled_package: CompiledPackage,
    network: &str
) -> Result<()> {
    let new_account_key = load_key(account_key_path);
    let json_rpc_url = format!("http://0.0.0.0:{}", 8080); // TODO: When account transaction API lands, use network arg here
    let factory = TransactionFactory::new(ChainId::test());
    println!("Connecting to {}", json_rpc_url);

    let client = BlockingClient::new(json_rpc_url);

    println!("Using Public Key {}", &new_account_key.public_key());
    let derived_address =
        AuthenticationKey::ed25519(&new_account_key.public_key()).derived_address();
    println!(
        "Sending txn from address {}",
        derived_address.to_hex_literal()
    );

    // Send a module transaction
    let seq_number = client
        .get_account(derived_address)?
        .into_inner()
        .ok_or_else(|| anyhow::anyhow!("missing AccountView"))?
        .sequence_number;
    let mut new_account = LocalAccount::new(derived_address, new_account_key, seq_number);
    send_module_transaction(&compiled_package, &mut new_account, &factory, network)?;
    Ok(())

}

pub fn send_module_transaction(
    compiled_package: &CompiledPackage,
    account: &mut LocalAccount,
    factory: &TransactionFactory,
    network: &str
) -> Result<()> {
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
            if !confirm_txn_executed(hash, network)? {
                return Err(anyhow!(
                    format!("Transaction wasn't executed: {:?}", publish_txn)
                ));
            }

        } else {
            println!("Skipping Module: {}", module_id);
        }
    }

    println!("Success!");
    Ok(())
}

pub fn send_txn(txn_bytes : Vec<u8>, network : &str) -> Result<String> {
    let path = format!("http://{}/transactions", network);
    let resp = ureq::post(path.as_str())
        .set("Content-Type", "application/vnd.bcs+signed_transaction")
        .send_bytes(&*txn_bytes)?;
    let json : serde_json::Value = resp.into_json()?;

    let hash = json["hash"].as_str().unwrap();
    Ok(hash.to_string())
}

pub fn confirm_txn_executed(hash : String, network: &str) -> Result<bool> {
    let path = format!("http://{}/transactions/{}", network, hash);
    let resp = ureq::get(path.as_str()).call()?;
    if resp.status() == 200 {
        return Ok(true);
    }
    Ok(false)
}
