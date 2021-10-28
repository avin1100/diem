// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{account, deploy, shared};
use anyhow::{Context, Result};
use diem_config::config::{NodeConfig, DEFAULT_PORT};
use diem_crypto::PrivateKey;
use diem_sdk::{
    client::{AccountAddress, BlockingClient},
    transaction_builder::TransactionFactory,
    types::LocalAccount,
};
use diem_types::{chain_id::ChainId, transaction::authenticator::AuthenticationKey};
use shared::Home;
use std::{collections::HashMap, path::Path, process::Command};

pub async fn handle(project_path: &Path) -> Result<()> {
    shared::generate_typescript_libraries(project_path)?;
    let home = Home::new(shared::get_home_path().as_path())?;

    let config = NodeConfig::load(&home.get_node_config_path()).with_context(|| {
        format!(
            "Failed to load NodeConfig from file: {:?}",
            home.get_node_config_path()
        )
    })?;
    let json_rpc_url = format!("http://0.0.0.0:{}", config.json_rpc.address.port());
    println!("Connecting to {}...", json_rpc_url);
    let client = BlockingClient::new(&json_rpc_url);
    let factory = TransactionFactory::new(ChainId::test());

    let new_account = create_test_account(&client, &home, &factory)?;
    create_receiver_account(&client, &home, &factory)?;
    deploy::handle(project_path).await?;

    run_deno_test(
        project_path,
        &config,
        json_rpc_url.as_str(),
        home.get_test_key_path(),
        new_account.address(),
    )
}

// Set up a new test account
fn create_test_account(
    client: &BlockingClient,
    home: &Home,
    factory: &TransactionFactory,
) -> Result<LocalAccount> {
    let mut root_account = account::get_root_account(client, home.get_root_key_path());
    // TODO: generate random key by using let new_account_key = generate_key::generate_key();
    let new_account_key = generate_key::load_key(home.get_latest_key_path());
    let public_key = new_account_key.public_key();
    let derived_address = AuthenticationKey::ed25519(&public_key).derived_address();
    let new_account = LocalAccount::new(derived_address, new_account_key, 0);
    account::create_account_onchain(&mut root_account, &new_account, factory, client)?;
    Ok(new_account)
}

// Set up a new test account
fn create_receiver_account(
    client: &BlockingClient,
    home: &Home,
    factory: &TransactionFactory,
) -> Result<LocalAccount> {
    let mut root_account = account::get_root_account(client, home.get_root_key_path());
    let receiver_account_key = generate_key::load_key(home.get_test_key_path());
    let public_key = receiver_account_key.public_key();
    let address = AuthenticationKey::ed25519(&public_key).derived_address();
    let receiver_account = LocalAccount::new(address, receiver_account_key, 0);
    account::create_account_onchain(&mut root_account, &receiver_account, factory, client)?;

    Ok(receiver_account)
}

// Run shuffle test using deno
fn run_deno_test(
    project_path: &Path,
    config: &NodeConfig,
    network: &str,
    key_path: &Path,
    sender_address: AccountAddress,
) -> Result<()> {
    let tests_path_string = project_path
        .join("e2e")
        .as_path()
        .to_string_lossy()
        .to_string();

    let mut filtered_envs: HashMap<String, String> = HashMap::new();
    filtered_envs.insert(
        String::from("PROJECT_PATH"),
        project_path.to_str().unwrap().to_string(),
    );
    filtered_envs.insert(
        String::from("SHUFFLE_HOME"),
        shared::get_shuffle_dir().to_str().unwrap().to_string(),
    );

    filtered_envs.insert(String::from("SENDER_ADDRESS"), sender_address.to_string());
    filtered_envs.insert(
        String::from("PRIVATE_KEY_PATH"),
        key_path.to_string_lossy().to_string(),
    );

    filtered_envs.insert(String::from("SHUFFLE_NETWORK"), network.to_string());

    Command::new("deno")
        .args([
            "test",
            "--unstable",
            tests_path_string.as_str(),
            "--allow-env=PROJECT_PATH,SHUFFLE_HOME,SHUFFLE_NETWORK,PRIVATE_KEY_PATH,SENDER_ADDRESS",
            "--allow-read",
            format!(
                "--allow-net=:{},:{}",
                DEFAULT_PORT,
                config.json_rpc.address.port()
            )
            .as_str(),
        ])
        .envs(&filtered_envs)
        .spawn()
        .expect("deno failed to start, is it installed? brew install deno")
        .wait()?;
    Ok(())
}
