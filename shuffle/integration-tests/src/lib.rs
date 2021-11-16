// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

mod helper;

use crate::helper::ShuffleTestHelper;
use forge::{AdminContext, AdminTest, Result, Test};
use smoke_test::scripts_and_modules::enable_open_publishing;
use tokio::runtime::Runtime;
use shuffle::{shared, shared::DevApiClient};
use url::Url;
use std::str::FromStr;
use shuffle::shared::Network;
use diem_sdk::types::LocalAccount;

pub struct SetMessageHelloBlockchain;

impl Test for SetMessageHelloBlockchain {
    fn name(&self) -> &'static str {
        "shuffle::set-message-hello-blockchain"
    }
}

impl AdminTest for SetMessageHelloBlockchain {
      fn run<'t>(&self, ctx: &mut AdminContext<'t>) -> Result<()> {
        let client = ctx.client();
        let dev_client = DevApiClient::new(reqwest::Client::new(), shared::Network::default().get_dev_api_url()?)?;

        let factory = ctx.chain_info().transaction_factory();
        enable_open_publishing(&client, &factory, ctx.chain_info().root_account())?;

        let helper = ShuffleTestHelper::new()?;
        helper.create_project()?;

        let new_account = ctx.random_account();
        let tc = ctx.chain_info().treasury_compliance_account();

        let rt = Runtime::new().unwrap();
        let handle = rt.handle().clone();
        handle.block_on(helper.create_accounts(tc, new_account, factory, dev_client))?;

        handle.block_on(helper.deploy_project(ctx.chain_info().rest_api()))?;

        shuffle::test::run_move_unit_tests(&helper.project_path())?;
        shuffle::test::run_deno_test(
          helper.home(),
          &helper.project_path(),
          &Network::new(
            "forge_test_network".to_string(),
            Url::from_str(ctx.chain_info().json_rpc())?, Url::from_str(ctx.chain_info().rest_api())?, None),
          helper.network_home().get_test_key_path(),
          helper.network_home().get_test_address()?,
        )
    }
}
