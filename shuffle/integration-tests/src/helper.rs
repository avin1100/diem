// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use diem_sdk::{
    types::account_config,
    transaction_builder::TransactionFactory, types::LocalAccount,
};
use shuffle::{
    account, deploy, new,
    shared::{Home, NetworkHome, LOCALHOST_NAME},
};
use std::{path::PathBuf, str::FromStr};
use tempfile::TempDir;
use url::Url;
use shuffle::shared::DevApiClient;

pub struct ShuffleTestHelper {
    home: Home,
    network_home: NetworkHome,
    tmp_dir: TempDir,
}

impl ShuffleTestHelper {
    pub fn new() -> Result<Self> {
        let tmp_dir = TempDir::new()?;
        let home = Home::new(tmp_dir.path())?;
        let network_home = home.get_network_home(LOCALHOST_NAME);
        Ok(Self {
            tmp_dir,
            home,
            network_home,
        })
    }

    pub fn home(&self) -> &Home {
        &self.home
    }

    pub fn network_home(&self) -> NetworkHome {
        self.home.get_network_home(LOCALHOST_NAME)
    }

    pub fn project_path(&self) -> PathBuf {
        self.tmp_dir.path().join("project")
    }

    pub async fn create_accounts(
        &self,
        treasury_account: &mut LocalAccount,
        new_account: LocalAccount,
        factory: TransactionFactory,
        client: DevApiClient,
    ) -> Result<()> {
        account::create_local_account(treasury_account, &new_account, &factory, &client).await
    }

    pub fn create_project(&self) -> Result<()> {
        new::handle(
            &self.home,
            new::DEFAULT_BLOCKCHAIN.to_string(),
            self.project_path(),
        )
    }

    pub async fn deploy_project(&self, dev_api_url: &str) -> Result<()> {
        let url = Url::from_str(dev_api_url)?;
        deploy::handle(&self.network_home, &self.project_path(), url).await
    }

    pub async fn get_tc_seq_num(&self, client: DevApiClient) -> Result<u64> {
        Ok(client.get_account_sequence_number(account_config::treasury_compliance_account_address()).await?)
    }
}
