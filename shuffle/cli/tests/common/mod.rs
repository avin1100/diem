// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use diem_sdk::{
    client::BlockingClient,
    crypto::ed25519::Ed25519PrivateKey,
    transaction_builder::TransactionFactory,
    types::{AccountKey, LocalAccount},
};
use forge::{AdminContext, ChainInfo};
use shuffle::{
    account, deploy,
    dev_api_client::DevApiClient,
    new, shared,
    shared::{Home, Network, NetworkHome, NetworksConfig},
};
use smoke_test::scripts_and_modules::enable_open_publishing;
use std::{
    collections::BTreeMap,
    convert::TryFrom,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};
use tokio::runtime::Runtime;
use url::Url;

pub struct ShuffleTestHelper {
    home: Home,
    network: Network,
    networks_config: NetworksConfig,
    network_home: NetworkHome,
    base_path: PathBuf,
}

const FORGE_NETWORK_NAME: &str = "forge";

impl ShuffleTestHelper {
    pub fn new(chain_info: &mut ChainInfo<'_>, base_path: &Path) -> Result<Self> {
        let home = Home::new(base_path)?;
        let network_home = home.new_network_home(FORGE_NETWORK_NAME);
        network_home.generate_paths_if_nonexistent()?;
        let network = Network::new(
            String::from(FORGE_NETWORK_NAME),
            Url::from_str(chain_info.json_rpc())?,
            Url::from_str(chain_info.rest_api())?,
            None,
        );
        let mut mapping: BTreeMap<String, Network> = BTreeMap::new();
        mapping.insert(FORGE_NETWORK_NAME.to_string(), network.clone());
        let networks_config = NetworksConfig::new(mapping);
        Ok(Self {
            home,
            network,
            networks_config,
            network_home,
            base_path: base_path.to_path_buf(),
        })
    }

    pub fn hardcoded_0x2416_account(
        network_home: &NetworkHome,
        client: &BlockingClient,
    ) -> Result<LocalAccount> {
        let private_key: Ed25519PrivateKey = bcs::from_bytes(shared::NEW_KEY_FILE_CONTENT)?;
        let key = AccountKey::from_private_key(private_key);

        let private_key_clone: Ed25519PrivateKey = bcs::from_bytes(shared::NEW_KEY_FILE_CONTENT)?;
        network_home.save_key_as_latest(private_key_clone)?;

        let address = key.authentication_key().derived_address();
        let account_view = client.get_account(address)?.into_inner();
        let seq_num = match account_view {
            Some(account_view) => account_view.sequence_number,
            None => 0,
        };
        Ok(LocalAccount::new(address, key, seq_num))
    }

    pub fn home(&self) -> &Home {
        &self.home
    }

    pub fn network(&self) -> &Network {
        &self.network
    }

    pub fn network_home(&self) -> &NetworkHome {
        &self.network_home
    }

    pub fn networks_config(&self) -> &NetworksConfig {
        &self.networks_config
    }

    pub fn project_path(&self) -> PathBuf {
        self.base_path.as_path().join("project")
    }

    pub async fn create_account(
        &self,
        treasury_account: &mut LocalAccount,
        new_account: &LocalAccount,
        factory: TransactionFactory,
        client: &DevApiClient,
    ) -> Result<()> {
        let bytes: &[u8] = &new_account.private_key().to_bytes();
        let private_key = Ed25519PrivateKey::try_from(bytes).map_err(anyhow::Error::new)?;
        self.network_home().save_key_as_latest(private_key)?;
        println!(
            "here is the latest path {:?}",
            self.network_home().get_latest_account_key_path()
        );
        self.network_home()
            .generate_latest_address_file(new_account.public_key())?;
        account::create_account_via_dev_api(treasury_account, new_account, &factory, client).await
    }

    pub fn create_project(&self) -> Result<()> {
        new::handle(
            &self.home,
            new::DEFAULT_BLOCKCHAIN.to_string(),
            self.project_path(),
        )
    }

    pub async fn deploy_project(
        &self,
        account: &mut LocalAccount,
        dev_api_url: &str,
    ) -> Result<()> {
        let url = Url::from_str(dev_api_url)?;
        let client = DevApiClient::new(reqwest::Client::new(), url)?;
        deploy::deploy(client, account, &self.project_path()).await
    }

    pub fn overwrite_networks_config_into_toml(
        &self,
        networks_config: NetworksConfig,
    ) -> Result<()> {
        let network_toml_path = self.home.get_shuffle_path().join("Networks.toml");
        let networks_config_string = toml::to_string_pretty(&networks_config)?;
        fs::write(network_toml_path, networks_config_string)?;
        Ok(())
    }
}

pub fn bootstrap_shuffle(
    ctx: &mut AdminContext<'_>,
    base_path: &Path,
) -> Result<ShuffleTestHelper> {
    let client = ctx.client();
    let dev_client = DevApiClient::new(
        reqwest::Client::new(),
        Url::from_str(ctx.chain_info().rest_api())?,
    )?;
    let factory = ctx.chain_info().transaction_factory();
    enable_open_publishing(&client, &factory, ctx.chain_info().root_account())?;

    let helper = ShuffleTestHelper::new(ctx.chain_info(), base_path)?;
    helper.create_project()?;

    let network_home = helper.network_home();
    // let mut account = ctx.random_account(); // TODO: Support arbitrary addresses
    let rt = Runtime::new().unwrap();
    let handle = rt.handle().clone();
    let mut account = ShuffleTestHelper::hardcoded_0x2416_account(network_home, &client)?;
    let tc = ctx.chain_info().treasury_compliance_account();

    handle.block_on(helper.create_account(tc, &account, factory, &dev_client))?;
    handle.block_on(helper.deploy_project(&mut account, ctx.chain_info().rest_api()))?;

    Ok(helper)
}
