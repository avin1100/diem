// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::shared::{
    send_transaction, Home, Network, NetworkHome, LOCALHOST_NAME, TROVE_TESTNET_NETWORK_NAME,
};
use anyhow::{anyhow, Context, Result};
use diem_config::config::NodeConfig;
use diem_crypto::PrivateKey;

use crate::shared;
use diem_infallible::duration_since_epoch;
use diem_sdk::client::FaucetClient;
use diem_sdk::{
    client::BlockingClient,
    transaction_builder::{Currency, TransactionFactory},
    types::LocalAccount,
};
use diem_types::{
    account_address::AccountAddress,
    account_config,
    chain_id::ChainId,
    transaction::{authenticator::AuthenticationKey, ScriptFunction, TransactionPayload},
};
use generate_key::load_key;
use move_core_types::{
    ident_str,
    language_storage::{ModuleId, TypeTag},
};
use std::{
    io,
    path::{Path, PathBuf},
};
use url::Url;

// Creates new account from randomly generated private/public key pair.
pub async fn handle(home: &Home, root: Option<PathBuf>, network: Network) -> Result<()> {
    check_node_deployed_if_localhost_used(home, &network)?;

    let network_home = NetworkHome::new(
        shared::get_shuffle_networks_path().as_path(),
        network.get_name(),
    );
    network_home.generate_specified_network_path_if_nonexistent()?;

    // if user already has account from key/address pair
    // check if user wants another key/address pair
    if network_home.get_latest_path().exists() {
        match user_wants_another_key(&network_home) {
            true => archive_current_files_in_latest(&network_home)?,
            false => return Ok(()),
        }
    }

    // generating paths ~/.shuffle/networks/{network_name}/latest
    // and ~/.shuffle/networks/{network_name}/address if they don't exist
    network_home.generate_network_accounts_path_if_nonexistent()?;
    network_home.generate_network_latest_path_if_nonexistent()?;


    let new_account = generate_new_account(&network_home)?;

    //creating account onchain based on network provided by user
    match network.get_name().as_str() {
        LOCALHOST_NAME => handle_account_creation_on_localhost(home, &network_home, root, &new_account),
        TROVE_TESTNET_NETWORK_NAME => {
            create_account_on_network(
                new_account,
                &network.get_faucet_url()?,
                &network.get_dev_api_url()?,
            )
            .await
        }
        _ => Ok(()),
    }
}

fn handle_account_creation_on_localhost(
    home: &Home,
    network_home: &NetworkHome,
    root: Option<PathBuf>,
    new_account: &LocalAccount,
) -> Result<()> {
    let config = NodeConfig::load(&home.get_validator_config_path()).with_context(|| {
        format!(
            "Failed to load NodeConfig from file: {:?}",
            home.get_validator_config_path()
        )
    })?;
    let json_rpc_url = format!("http://0.0.0.0:{}", config.json_rpc.address.port());
    println!("Connecting to {}...", json_rpc_url);
    let client = BlockingClient::new(json_rpc_url);
    let factory = TransactionFactory::new(ChainId::test());


    if let Some(input_root_key) = root {
        network_home.save_root_key(input_root_key.as_path())?
    }

    let mut root_account = get_root_account(&client, home.get_root_key_path());
    create_account_on_localhost(&mut root_account, new_account, &factory, &client)?;

    network_home.generate_shuffle_test_path_if_nonexistent()?;
    let test_account = generate_test_account(network_home)?;
    create_account_on_localhost(&mut root_account, &test_account, &factory, &client)
}

pub fn create_account_on_localhost(
    root_account: &mut LocalAccount,
    new_account: &LocalAccount,
    factory: &TransactionFactory,
    client: &BlockingClient,
) -> Result<()> {
    println!("Creating a new account onchain...");
    if client
        .get_account(new_account.address())
        .unwrap()
        .into_inner()
        .is_some()
    {
        println!("Account already exists: {}", new_account.address());
    } else {
        let create_new_account_txn = root_account.sign_with_transaction_builder(factory.payload(
            encode_create_parent_vasp_account_script_function(
                Currency::XUS.type_tag(),
                0,
                new_account.address(),
                new_account.authentication_key().prefix().to_vec(),
                vec![],
                false,
            ),
        ));
        send_transaction(client, create_new_account_txn)?;
        println!("Successfully created account {}", new_account.address());
    }
    println!(
        "Private key: {}",
        ::hex::encode(new_account.private_key().to_bytes())
    );
    println!("Public key: {}", new_account.public_key());
    Ok(())
}

async fn create_account_on_network(
    new_account: LocalAccount,
    faucet_base_url: &Url,
    json_rpc_url: &Url,
) -> Result<()> {
    let faucet_account_creation_endpoint = faucet_base_url.join("accounts")?;
    let faucet_client = FaucetClient::new(
        faucet_account_creation_endpoint.to_string(),
        json_rpc_url.to_string(),
    );
    tokio::task::spawn_blocking(move || {
        faucet_client
            .create_account(new_account.authentication_key(), "XUS")
            .unwrap()
    })
    .await
    .unwrap();

    Ok(())
}

fn generate_new_account(network_home: &NetworkHome) -> Result<LocalAccount> {
    // generating and saving key/address pair for new account
    let new_account_key = network_home.generate_key_file()?;
    network_home.generate_address_file(&new_account_key.public_key())?;
    Ok(LocalAccount::new(
        AuthenticationKey::ed25519(&new_account_key.public_key()).derived_address(),
        new_account_key,
        0,
    ))
}

fn generate_test_account(network_home: &NetworkHome) -> Result<LocalAccount> {
    // generating and saving key/address pair for new account
    let test_key = network_home.generate_testkey_file()?;
    let public_test_key = test_key.public_key();
    network_home.generate_testkey_address_file(&test_key.public_key())?;
    Ok(LocalAccount::new(
        AuthenticationKey::ed25519(&public_test_key).derived_address(),
        test_key,
        0,
    ))
}

fn archive_current_files_in_latest(network_home: &NetworkHome) -> Result<()> {
    let time = duration_since_epoch();
    let archive_dir = network_home.create_archive_dir(time)?;
    network_home.archive_old_key(&archive_dir)?;
    network_home.archive_old_address(&archive_dir)
}

fn user_wants_another_key(network_home: &NetworkHome) -> bool {
    let key_path = network_home.get_latest_key_path();
    let prev_key = generate_key::load_key(&key_path);
    println!(
        "Private Key already exists: {}",
        ::hex::encode(prev_key.to_bytes())
    );
    println!("Are you sure you want to generate a new key? [y/n]");

    let mut user_response = String::new();
    io::stdin()
        .read_line(&mut user_response)
        .expect("Failed to read line");
    let user_response = user_response.trim().to_owned();
    delegate_user_response(user_response.as_str())
}

fn delegate_user_response(user_response: &str) -> bool {
    if user_response != "y" && user_response != "n" {
        println!("Please restart and enter either y or n");
        return false;
    } else if user_response == "n" {
        return false;
    }

    true
}

pub fn get_root_account(client: &BlockingClient, root_key_path: &Path) -> LocalAccount {
    let root_account_key = load_key(root_key_path);

    let root_seq_num = client
        .get_account(account_config::treasury_compliance_account_address())
        .unwrap()
        .into_inner()
        .unwrap()
        .sequence_number;
    LocalAccount::new(
        account_config::treasury_compliance_account_address(),
        root_account_key,
        root_seq_num,
    )
}

fn check_node_deployed_if_localhost_used(home: &Home, network: &Network) -> Result<()> {
    match network.get_name().as_str() {
        LOCALHOST_NAME => match home.get_node_config_path().is_dir() {
            true => Ok(()),
            false => Err(anyhow!(
                "A node hasn't been created yet! Run shuffle node first"
            )),
        },
        _ => Ok(()),
    }
}

fn encode_create_parent_vasp_account_script_function(
    coin_type: TypeTag,
    sliding_nonce: u64,
    new_account_address: AccountAddress,
    auth_key_prefix: Vec<u8>,
    human_name: Vec<u8>,
    add_all_currencies: bool,
) -> TransactionPayload {
    TransactionPayload::ScriptFunction(ScriptFunction::new(
        ModuleId::new(
            AccountAddress::new([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
            ident_str!("AccountCreationScripts").to_owned(),
        ),
        ident_str!("create_parent_vasp_account").to_owned(),
        vec![coin_type],
        vec![
            bcs::to_bytes(&sliding_nonce).unwrap(),
            bcs::to_bytes(&new_account_address).unwrap(),
            bcs::to_bytes(&auth_key_prefix).unwrap(),
            bcs::to_bytes(&human_name).unwrap(),
            bcs::to_bytes(&add_all_currencies).unwrap(),
        ],
    ))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::shared;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_check_node_deployed_if_localhost_used() {
        let tmpdir = tempdir().unwrap();
        let dir_path = tmpdir.path();
        let home = Home::new(dir_path).unwrap();
        assert_eq!(
            check_node_deployed_if_localhost_used(&home, &shared::Network::localhost().unwrap())
                .is_err(),
            true
        );
        fs::create_dir_all(dir_path.join(".shuffle/nodeconfig")).unwrap();
        assert_eq!(
            check_node_deployed_if_localhost_used(&home, &shared::Network::localhost().unwrap())
                .is_err(),
            false
        );
    }

    #[test]
    fn test_delegate_user_response() {
        assert_eq!(delegate_user_response("a"), false);
        assert_eq!(delegate_user_response("n"), false);
        assert_eq!(delegate_user_response("y"), true);
    }
}
