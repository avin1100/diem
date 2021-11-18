// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

mod common;

use forge::{
    forge_main, AdminContext, AdminTest, ForgeConfig, LocalFactory, Options, Result, Test,
};

use common::{bootstrap_shuffle, ShuffleTestHelper};
use smoke_test::scripts_and_modules::enable_open_publishing;
use std::{os::unix::prelude::ExitStatusExt, process::ExitStatus};
use tempfile::tempdir;

const HOME_PATH: &str = "--home-path";
pub const BINARY: &str = env!("CARGO_BIN_EXE_shuffle");

fn main() -> Result<()> {
    let tests = ForgeConfig::default().with_admin_tests(&[
        &TransactionsWithoutProjectFolder,
        &TransactionsWithoutAccount,
        &TransactionsWithNetworkRawAddressFlags,
    ]);
    let options = Options::from_args();
    forge_main(tests, LocalFactory::from_workspace()?, &options)
}

pub struct TransactionsWithoutProjectFolder;

impl Test for TransactionsWithoutProjectFolder {
    fn name(&self) -> &'static str {
        "shuffle::transactions-without-project-folder"
    }
}

impl AdminTest for TransactionsWithoutProjectFolder {
    fn run<'t>(&self, _ctx: &mut AdminContext<'t>) -> Result<()> {
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();
        let base_path_string = base_path.to_string_lossy().to_string();
        let output = std::process::Command::new(BINARY)
            .args([
                HOME_PATH,
                base_path_string.as_str(),
                "transactions",
                "--network",
                "forge",
            ])
            .output()?;
        let std_err = String::from_utf8(output.stderr)?;
        assert_eq!(
            "Error: A project hasn't been created yet. Run shuffle new first\n",
            std_err
        );
        assert_eq!(output.status, ExitStatus::from_raw(256));
        Ok(())
    }
}

pub struct TransactionsWithoutAccount;

impl Test for TransactionsWithoutAccount {
    fn name(&self) -> &'static str {
        "shuffle::transactions-without-account"
    }
}

impl AdminTest for TransactionsWithoutAccount {
    fn run<'t>(&self, ctx: &mut AdminContext<'t>) -> Result<()> {
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();
        let base_path_string = base_path.to_string_lossy().to_string();

        let helper = ShuffleTestHelper::new(ctx.chain_info(), base_path)?;
        let client = ctx.client();
        let factory = ctx.chain_info().transaction_factory();
        enable_open_publishing(&client, &factory, ctx.chain_info().root_account())?;
        helper.create_project()?;
        helper.overwrite_networks_config_into_toml(helper.networks_config().clone())?;
        let output = std::process::Command::new(BINARY)
            .args([
                HOME_PATH,
                base_path_string.as_str(),
                "transactions",
                "--network",
                "forge",
            ])
            .output()?;
        let std_err = String::from_utf8(output.stderr)?;

        assert_eq!(
            "Error: An account hasn't been created yet! Run shuffle account first\n",
            std_err
        );
        assert_eq!(output.status, ExitStatus::from_raw(256));
        Ok(())
    }
}

pub struct TransactionsWithNetworkRawAddressFlags;

impl Test for TransactionsWithNetworkRawAddressFlags {
    fn name(&self) -> &'static str {
        "shuffle::transactions-with-networks-raw-and-address-flags"
    }
}

impl AdminTest for TransactionsWithNetworkRawAddressFlags {
    fn run<'t>(&self, ctx: &mut AdminContext<'t>) -> Result<()> {
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();
        let helper = bootstrap_shuffle(ctx, base_path)?;
        let base_path_string = base_path.to_string_lossy().to_string();

        helper.overwrite_networks_config_into_toml(helper.networks_config().clone())?;
        let output = std::process::Command::new(BINARY)
            .args([
                HOME_PATH,
                base_path_string.as_str(),
                "transactions",
                "--raw",
                "--network",
                "forge",
                "--address",
                "24163AFCC6E33B0A9473852E18327FA9",
            ])
            .output()?;
        let std_out = String::from_utf8(output.stdout)?;
        assert_modules_appear_in_txns(std_out.as_str(), true);
        assert_eq!(output.status, ExitStatus::from_raw(0));
        Ok(())
    }
}

fn assert_modules_appear_in_txns(std_out: &str, raw: bool) {
    let space = match raw {
        true => "",
        false => " ",
    };
    assert_eq!(
        std_out.contains(format!(r#""sequence_number":{}"0""#, space).as_str()),
        true
    );
    assert_eq!(
        std_out.contains(format!(r#""sequence_number":{}"1""#, space).as_str()),
        true
    );
    assert_eq!(
        std_out.contains(format!(r#""sequence_number":{}"2""#, space).as_str()),
        true
    );
    assert_eq!(
        std_out.contains(format!(r#""name":{}"Message""#, space).as_str()),
        true
    );
    assert_eq!(
        std_out.contains(format!(r#""name":{}"TestNFT""#, space).as_str()),
        true
    );
    assert_eq!(
        std_out.contains(format!(r#""name":{}"NFT""#, space).as_str()),
        true
    );
}
