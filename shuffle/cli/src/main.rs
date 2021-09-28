// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use std::path::PathBuf;
use structopt::StructOpt;

mod account;
mod deploy;
mod new;
mod node;
mod utils;

pub fn main() -> Result<()> {
    let subcommand = Subcommand::from_args();
    match subcommand {
        Subcommand::New { blockchain, path } => new::handle(blockchain, path),
        Subcommand::Node { project_path } => node::handle(project_path.as_path()),
        Subcommand::Deploy {
            project_path,
            account_key_path,
        } => deploy::handle(project_path, account_key_path),
        Subcommand::Account {
            project_path,
            account_key_path,
            random,
        } => account::handle(project_path, account_key_path, random),
    }
}

#[derive(Debug, StructOpt)]
#[structopt(name = "shuffle", about = "CLI frontend for Shuffle toolset")]
pub enum Subcommand {
    #[structopt(about = "Creates a new shuffle project for Move development")]
    New {
        #[structopt(short, long, default_value = new::DEFAULT_BLOCKCHAIN)]
        blockchain: String,

        /// Path to destination dir
        #[structopt(parse(from_os_str))]
        path: PathBuf,
    },
    #[structopt(about = "Runs a local devnet with prefunded accounts")]
    Node { project_path: PathBuf },
    #[structopt(about = "Publishes a move module under an account")]
    Deploy {
        project_path: PathBuf,
        account_key_path: PathBuf,
    },
    #[structopt(about = "Create a new account")]
    Account {
        project_path: PathBuf,
        account_key_path: Option<PathBuf>,
        #[structopt(short)]
        random: bool
    },
}
