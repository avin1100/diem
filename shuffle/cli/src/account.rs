// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0
use anyhow::{Result};
use diem_config::config::NodeConfig;
use diem_sdk::{
    client::BlockingClient,
    crypto::PrivateKey,
    transaction_builder::{Currency, TransactionFactory},
    types::{
        LocalAccount,
    },
};
use diem_types::{
    account_config,
    chain_id::ChainId, transaction::authenticator::AuthenticationKey,
};
use generate_key::load_key;
use shuffle_transaction_builder::framework::{
    encode_create_parent_vasp_account_script_function,
};
use std::{
    path::{Path, PathBuf},
};



pub fn handle(project_dir: PathBuf) -> Result<()> {

    let project_ptr = project_dir.as_path();
    let json_rpc_url = generate_json_rpc_url(project_ptr);

    println!("Connecting to {}...", json_rpc_url);

    let client = BlockingClient::new(json_rpc_url);
    let mut root_account = generate_root_account(project_ptr, &client);
    let new_account = generate_new_account();

    println!("======new account {}", new_account.address());

    // Create a new account.
    print!("Create a new ParentVASP account (we cannot create a regular account right now)...");
    let create_new_account_txn = root_account.sign_with_transaction_builder(
        TransactionFactory::new(ChainId::test()).payload(
            encode_create_parent_vasp_account_script_function(
                Currency::XUS.type_tag(),
                0,
                new_account.address(),
                new_account.authentication_key().prefix().to_vec(),
                vec![],
                false,
            ),
        ),
    );
    send(&client, create_new_account_txn)?;
    println!("Success!");
    Ok(())
}


pub fn generate_json_rpc_url(project_dir: &Path) -> String {
    let config_path = project_dir.join("nodeconfig/0").join("node.yaml");
    let config = NodeConfig::load(&config_path).expect("Failed to load \
    NodeConfig from given project directory");
    let json_rpc_url = format!("http://0.0.0.0:{}", config.json_rpc.address.port());
    return json_rpc_url
}

pub fn generate_root_account(project_dir: &Path, client: &BlockingClient) -> LocalAccount {
    let root_key_path = project_dir.join("nodeconfig").join("mint.key");
    let root_account_key = load_key(root_key_path);


    let root_seq_num = client
        .get_account(account_config::treasury_compliance_account_address())
        .unwrap()
        .into_inner()
        .unwrap()
        .sequence_number;

    let root_account = LocalAccount::new(
        account_config::treasury_compliance_account_address(),
        root_account_key,
        root_seq_num,
    );
    return root_account;
}

pub fn generate_new_account() -> LocalAccount {
    let new_account_key = generate_key::generate_key();
    let new_account = LocalAccount::new(
        AuthenticationKey::ed25519(&new_account_key.public_key()).derived_address(),
        new_account_key,
        0,
    );

    return new_account;
}
