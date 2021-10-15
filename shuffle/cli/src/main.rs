// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use std::path::PathBuf;
use structopt::StructOpt;

mod account;
mod console;
mod deploy;
mod new;
mod node;
mod shared;
mod test;

pub fn main() -> Result<()> {
    let subcommand = Subcommand::from_args();
    match subcommand {
        Subcommand::New { blockchain, path } => new::handle(blockchain, path),
        Subcommand::Node {} => node::handle(),
        Subcommand::Deploy { project_path } => deploy::handle(project_path.as_path()),
        Subcommand::Account { root } => account::handle(root),
        Subcommand::Console { project_path } => console::handle(project_path.as_path()),
        Subcommand::Test { project_path } => test::handle(project_path.as_path()),
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
    Node {},
    #[structopt(about = "Publishes a move module under an account")]
    Deploy { project_path: PathBuf },
    #[structopt(about = "Creates new account with randomly generated private/public key")]
    Account {
        #[structopt(short, long, help = "Creates account from mint.key passed in by user")]
        root: Option<PathBuf>,
    },
    #[structopt(about = "Starts a REPL for onchain inspection")]
    Console { project_path: PathBuf },
    #[structopt(about = "Runs end to end .ts tests")]
    Test { project_path: PathBuf },
}
