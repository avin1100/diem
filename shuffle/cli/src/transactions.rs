use crate::shared::{get_home_path, Home};
use anyhow::Result;
use diem_sdk::client::BlockingClient;
use diem_types::account_address::AccountAddress;
use std::{cmp::max, fs, str::FromStr};

pub fn handle(network: String) -> Result<()> {
    let home = Home::new(get_home_path().as_path())?;
    let json_rpc_url = format!("http://0.0.0.0:{}", 8080); //pass in network here
    println!("Connecting to {}...", json_rpc_url);
    let client = BlockingClient::new(json_rpc_url);
    let address_str = fs::read_to_string(home.get_latest_address_path())?;
    let address = AccountAddress::from_str(address_str.as_str())?;

    let seq_number = client
        .get_account(address)?
        .into_inner()
        .ok_or_else(|| anyhow::anyhow!("missing AccountView"))?
        .sequence_number;

    let potential_neg_number = seq_number as i64 - 10;
    let transactions = client
        .get_account_transactions(
            address,
            max(0, potential_neg_number) as u64,
            seq_number + 10,
            true,
        )
        .unwrap()
        .into_inner();

    println!("{:#?}", transactions);
    Ok(())
}
