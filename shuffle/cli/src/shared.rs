// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0
use anyhow::{anyhow, Result};
use diem_api_types::mime_types;
use diem_crypto::ed25519::{Ed25519PrivateKey, Ed25519PublicKey};
use diem_sdk::client::{AccountAddress, BlockingClient};
use diem_types::transaction::authenticator::AuthenticationKey;
use directories::BaseDirs;
use move_package::compilation::compiled_package::CompiledPackage;
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_generate as serdegen;
use serde_generate::SourceInstaller;
use serde_json::Value;
use serde_reflection::Registry;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};
use transaction_builder_generator as buildgen;
use transaction_builder_generator::SourceInstaller as BuildgenSourceInstaller;
use url::Url;

pub const MAIN_PKG_PATH: &str = "main";
const NEW_KEY_FILE_CONTENT: &[u8] = include_bytes!("../new_account.key");
const DIEM_ACCOUNT_TYPE: &str = "0x1::DiemAccount::DiemAccount";

pub const LOCALHOST_NAME: &str = "localhost";
pub const TROVE_TESTNET_NETWORK_NAME: &str = "trove_testnet";

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct ProjectConfig {
    blockchain: String,
}

impl ProjectConfig {
    pub fn new(blockchain: String) -> Self {
        Self { blockchain }
    }
}

pub fn get_home_path() -> PathBuf {
    BaseDirs::new()
        .expect("Unable to deduce base directory for OS")
        .home_dir()
        .to_path_buf()
}

pub fn read_project_config(project_path: &Path) -> Result<ProjectConfig> {
    let config_string = fs::read_to_string(project_path.join("Shuffle").with_extension("toml"))?;
    let read_config: ProjectConfig = toml::from_str(config_string.as_str())?;
    Ok(read_config)
}

/// Send a transaction to the blockchain through the blocking client.
pub fn send_transaction(
    client: &BlockingClient,
    tx: diem_types::transaction::SignedTransaction,
) -> Result<()> {
    use diem_json_rpc_types::views::VMStatusView;

    client.submit(&tx)?;
    let status = client
        .wait_for_signed_transaction(&tx, Some(std::time::Duration::from_secs(60)), None)?
        .into_inner()
        .vm_status;
    if status != VMStatusView::Executed {
        return Err(anyhow::anyhow!("transaction execution failed: {}", status));
    }
    Ok(())
}

/// Checks the current directory, and then parent directories for a Shuffle.toml
/// file to indicate the base project directory.
pub fn get_shuffle_project_path(cwd: &Path) -> Result<PathBuf> {
    let mut path: PathBuf = PathBuf::from(cwd);
    let project_file = Path::new("Shuffle.toml");

    loop {
        path.push(project_file);

        if path.is_file() {
            path.pop();
            return Ok(path);
        }

        if !(path.pop() && path.pop()) {
            return Err(anyhow::anyhow!(
                "unable to find Shuffle.toml; are you in a Shuffle project?"
            ));
        }
    }
}

pub fn get_filtered_envs_for_deno(
    home: &Home,
    project_path: &Path,
    network: &Network,
    key_path: &Path,
    sender_address: AccountAddress,
) -> Result<HashMap<String, String>> {
    let mut filtered_envs: HashMap<String, String> = HashMap::new();
    filtered_envs.insert(
        String::from("PROJECT_PATH"),
        project_path.to_string_lossy().to_string(),
    );
    filtered_envs.insert(
        String::from("SHUFFLE_BASE_NETWORKS_PATH"),
        home.get_networks_path().to_string_lossy().to_string(),
    );
    filtered_envs.insert(
        String::from("SENDER_ADDRESS"),
        sender_address.to_hex_literal(),
    );
    filtered_envs.insert(
        String::from("PRIVATE_KEY_PATH"),
        key_path.to_string_lossy().to_string(),
    );

    filtered_envs.insert(String::from("SHUFFLE_NETWORK_NAME"), network.get_name());
    filtered_envs.insert(
        String::from("SHUFFLE_NETWORK_DEV_API_URL"),
        network.get_dev_api_url()?.to_string(),
    );
    Ok(filtered_envs)
}

pub struct DevApiClient {
    client: Client,
    network: Url,
}

// Client that will make GET and POST requests based off of Dev API
impl DevApiClient {
    pub fn new(client: Client, network: Url) -> Result<Self> {
        Ok(Self { client, network })
    }

    pub async fn get_transactions_by_hash(&self, hash: &str) -> Result<Response> {
        let path = self
            .network
            .join(format!("transactions/{}", hash).as_str())?;
        Ok(self.client.get(path.as_str()).send().await?)
    }

    pub async fn post_transactions(&self, txn_bytes: Vec<u8>) -> Result<Response> {
        let path = self.network.join("transactions")?;
        Ok(self
            .client
            .post(path.as_str())
            .header("Content-Type", mime_types::BCS_SIGNED_TRANSACTION)
            .body(txn_bytes)
            .send()
            .await?)
    }

    async fn get_account_resources(&self, address: AccountAddress) -> Result<Response> {
        let path = self
            .network
            .join(format!("accounts/{}/resources", address.to_hex_literal()).as_str())?;
        Ok(self.client.get(path.as_str()).send().await?)
    }

    pub async fn get_account_sequence_number(&self, address: AccountAddress) -> Result<u64> {
        let resp = self.get_account_resources(address).await?;
        DevApiClient::check_accounts_resources_response_status_code(&resp.status())?;
        let json: Vec<Value> = serde_json::from_str(resp.text().await?.as_str())?;
        DevApiClient::parse_json_for_account_seq_num(json)
    }

    fn check_accounts_resources_response_status_code(status_code: &StatusCode) -> Result<()> {
        match status_code == &StatusCode::from_u16(200)? {
            true => Ok(()),
            false => Err(anyhow!(
                "Failed to get account resources with provided address"
            )),
        }
    }

    fn parse_json_for_account_seq_num(json_objects: Vec<Value>) -> Result<u64> {
        let mut seq_number_string = "";
        for object in &json_objects {
            if object["type"] == DIEM_ACCOUNT_TYPE {
                seq_number_string = object["value"]["sequence_number"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Invalid sequence number string"))?;
                break;
            };
        }
        let seq_number: u64 = seq_number_string.parse()?;
        Ok(seq_number)
    }

    pub async fn get_account_transactions_response(
        &self,
        address: AccountAddress,
        start: u64,
        limit: u64,
    ) -> Result<Response> {
        let path = self
            .network
            .join(format!("accounts/{}/transactions", address).as_str())?;
        Ok(self
            .client
            .get(path.as_str())
            .query(&[("start", start.to_string().as_str())])
            .query(&[("limit", limit.to_string().as_str())])
            .send()
            .await?)
    }
}

pub struct NetworkHome {
    network_base_path: PathBuf,
    accounts_path: PathBuf,
    latest_account_path: PathBuf,
    latest_account_key_path: PathBuf,
    latest_account_address_path: PathBuf,
    test_path: PathBuf,
    test_key_path: PathBuf,
    test_key_address_path: PathBuf,
}

impl NetworkHome {
    pub fn new(network_base_path: &Path) -> Self {
        NetworkHome {
            network_base_path: network_base_path.to_path_buf(),
            accounts_path: network_base_path.join("accounts"),
            latest_account_path: network_base_path.join("accounts/latest"),
            latest_account_key_path: network_base_path.join("accounts/latest/dev.key"),
            latest_account_address_path: network_base_path.join("accounts/latest/address"),
            test_path: network_base_path.join("accounts/test"),
            test_key_path: network_base_path.join("accounts/test/dev.key"),
            test_key_address_path: network_base_path.join("accounts/test/address"),
        }
    }

    pub fn get_latest_account_path(&self) -> &Path {
        &self.latest_account_path
    }

    pub fn get_latest_account_key_path(&self) -> &Path {
        &self.latest_account_key_path
    }

    pub fn get_latest_account_address_path(&self) -> &Path {
        &self.latest_account_address_path
    }

    pub fn get_accounts_path(&self) -> &Path {
        &self.accounts_path
    }

    pub fn get_test_key_path(&self) -> &Path {
        &self.test_key_path
    }

    #[allow(dead_code)]
    pub fn get_test_address(&self) -> Result<AccountAddress> {
        let address_str = std::fs::read_to_string(&self.test_key_address_path)?;
        AccountAddress::from_str(address_str.as_str()).map_err(anyhow::Error::new)
    }

    pub fn create_archive_dir(&self, time: Duration) -> Result<PathBuf> {
        let archived_dir = self.accounts_path.join(time.as_secs().to_string());
        fs::create_dir(&archived_dir)?;
        Ok(archived_dir)
    }

    pub fn archive_old_key(&self, archived_dir: &Path) -> Result<()> {
        let old_key_path = self.latest_account_key_path.as_path();
        let archived_key_path = archived_dir.join("dev.key");
        fs::copy(old_key_path, archived_key_path)?;
        Ok(())
    }

    pub fn archive_old_address(&self, archived_dir: &Path) -> Result<()> {
        let old_address_path = self.latest_account_address_path.as_path();
        let archived_address_path = archived_dir.join("address");
        fs::copy(old_address_path, archived_address_path)?;
        Ok(())
    }

    pub fn generate_network_accounts_path_if_nonexistent(&self) -> Result<()> {
        if !self.accounts_path.is_dir() {
            fs::create_dir(self.accounts_path.as_path())?;
        }
        Ok(())
    }

    pub fn generate_network_latest_account_path_if_nonexistent(&self) -> Result<()> {
        if !self.latest_account_path.is_dir() {
            fs::create_dir(self.latest_account_path.as_path())?;
        }
        Ok(())
    }

    pub fn generate_network_base_path_if_nonexistent(&self) -> Result<()> {
        if !self.network_base_path.is_dir() {
            fs::create_dir_all(self.network_base_path.as_path())?;
        }
        Ok(())
    }

    pub fn generate_key_file(&self) -> Result<Ed25519PrivateKey> {
        // Using NEW_KEY_FILE for now due to hard coded address in
        // /diem/shuffle/move/examples/main/sources/move.toml
        fs::write(self.latest_account_key_path.as_path(), NEW_KEY_FILE_CONTENT)?;
        Ok(generate_key::load_key(
            self.latest_account_key_path.as_path(),
        ))
    }

    pub fn generate_address_file(&self, public_key: &Ed25519PublicKey) -> Result<()> {
        let address = AuthenticationKey::ed25519(public_key).derived_address();
        let address_filepath = self.latest_account_address_path.as_path();
        let mut file = File::create(address_filepath)?;
        file.write_all(address.to_string().as_ref())?;
        Ok(())
    }

    pub fn generate_shuffle_test_path_if_nonexistent(&self) -> Result<()> {
        if !self.test_path.is_dir() {
            fs::create_dir_all(self.test_path.as_path())?;
        }
        Ok(())
    }

    pub fn generate_testkey_file(&self) -> Result<Ed25519PrivateKey> {
        Ok(generate_key::generate_and_save_key(
            self.test_key_path.as_path(),
        ))
    }

    pub fn generate_testkey_address_file(&self, public_key: &Ed25519PublicKey) -> Result<()> {
        let address = AuthenticationKey::ed25519(public_key).derived_address();
        let address_filepath = self.test_key_address_path.as_path();
        let mut file = File::create(address_filepath)?;
        file.write_all(address.to_string().as_ref())?;
        Ok(())
    }

    pub fn save_root_key(&self, root_key_path: &Path) -> Result<()> {
        let new_root_key = generate_key::load_key(root_key_path);
        generate_key::save_key(new_root_key, self.latest_account_key_path.as_path());
        Ok(())
    }

    pub fn check_account_path_exists(&self) -> Result<()> {
        match self.accounts_path.is_dir() {
            true => Ok(()),
            false => Err(anyhow!(
                "An account hasn't been created yet! Run shuffle account first"
            )),
        }
    }
}

// Contains all the commonly used paths in shuffle/cli
pub struct Home {
    shuffle_path: PathBuf,
    networks_path: PathBuf,
    networks_config_path: PathBuf,
    node_config_path: PathBuf,
    root_key_path: PathBuf,
    validator_config_path: PathBuf,
    validator_log_path: PathBuf,
}

impl Home {
    pub fn new(home_path: &Path) -> Result<Self> {
        Ok(Self {
            shuffle_path: home_path.join(".shuffle"),
            networks_path: home_path.join(".shuffle/networks"),
            networks_config_path: home_path.join(".shuffle/Networks.toml"),
            node_config_path: home_path.join(".shuffle/nodeconfig"),
            root_key_path: home_path.join(".shuffle/nodeconfig/mint.key"),
            validator_log_path: home_path.join(".shuffle/nodeconfig/validator.log"),
            validator_config_path: home_path.join(".shuffle/nodeconfig/0/node.yaml"),
        })
    }

    pub fn get_shuffle_path(&self) -> &Path {
        &self.shuffle_path
    }

    pub fn get_networks_path(&self) -> &Path {
        &self.networks_path
    }

    pub fn get_root_key_path(&self) -> &Path {
        &self.root_key_path
    }

    pub fn get_node_config_path(&self) -> &Path {
        &self.node_config_path
    }

    pub fn get_validator_config_path(&self) -> &Path {
        &self.validator_config_path
    }

    pub fn get_validator_log_path(&self) -> &Path {
        &self.validator_log_path
    }

    pub fn get_network_home(&self, network_name: &str) -> NetworkHome {
        NetworkHome::new(self.networks_path.join(network_name).as_path())
    }

    pub fn read_networks_toml(&self) -> Result<NetworksConfig> {
        self.check_networks_toml_exists()?;
        let network_toml_contents = fs::read_to_string(self.networks_config_path.as_path())?;
        let network_toml: NetworksConfig = toml::from_str(network_toml_contents.as_str())?;
        Ok(network_toml)
    }

    fn check_networks_toml_exists(&self) -> Result<()> {
        match self.networks_config_path.exists() {
            true => Ok(()),
            false => Err(anyhow!(
                "A project hasn't been created yet. Run shuffle new first"
            )),
        }
    }

    pub fn read_genesis_waypoint(&self) -> Result<String> {
        fs::read_to_string(self.node_config_path.join("waypoint.txt")).map_err(anyhow::Error::new)
    }

    pub fn write_default_networks_config_into_toml_if_nonexistent(&self) -> Result<()> {
        if !&self.networks_config_path.exists() {
            let networks_config_string = toml::to_string_pretty(&NetworksConfig::default())?;
            fs::write(&self.networks_config_path, networks_config_string)?;
        }
        Ok(())
    }

    pub fn generate_shuffle_path_if_nonexistent(&self) -> Result<()> {
        if !self.shuffle_path.exists() {
            // creates .shuffle folder which will contain localhost nodeconfig,
            // Networks.toml, and account key/address pairs of each network
            fs::create_dir(&self.shuffle_path)?;
        }
        Ok(())
    }

    pub fn get_network_struct_from_toml(&self, network: &str) -> Result<Network> {
        self.read_networks_toml()?.get(network)
    }
}

pub fn normalized_network_url(home: &Home, network: Option<String>) -> Result<Url> {
    match network {
        Some(input) => Ok(home.read_networks_toml()?.get(input.as_str())?.dev_api_url),
        None => Ok(home.read_networks_toml()?.get(LOCALHOST_NAME)?.dev_api_url),
    }
}

pub fn normalized_network_name(network: Option<String>) -> String {
    match network {
        Some(net) => net,
        None => String::from(LOCALHOST_NAME),
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct NetworksConfig {
    networks: BTreeMap<String, Network>,
}

impl NetworksConfig {
    pub fn get(&self, network_name: &str) -> Result<Network> {
        Ok(self
            .networks
            .get(network_name)
            .ok_or_else(|| anyhow!("Please add specified network to the ~/.shuffle/Networks.json"))?
            .clone())
    }
}

impl Default for NetworksConfig {
    fn default() -> Self {
        let mut network_map = BTreeMap::new();
        network_map.insert(LOCALHOST_NAME.to_string(), Network::default());
        NetworksConfig {
            networks: network_map,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct Network {
    name: String,
    json_rpc_url: Url,
    dev_api_url: Url,
    faucet_url: Option<Url>,
}

impl Network {
    pub fn get_name(&self) -> String {
        String::from(&self.name)
    }

    pub fn get_json_rpc_url(&self) -> Result<Url> {
        Ok(Url::from_str(self.json_rpc_url.as_str())?)
    }

    pub fn get_dev_api_url(&self) -> Result<Url> {
        Ok(Url::from_str(self.dev_api_url.as_str())?)
    }

    pub fn get_faucet_url(&self) -> Result<Url> {
        match &self.faucet_url {
            Some(faucet) => Ok(Url::from_str(faucet.as_str())?),
            None => Err(anyhow!("This network doesn't have a faucet url")),
        }
    }
}

impl Default for Network {
    fn default() -> Self {
        Network {
            name: String::from(LOCALHOST_NAME),
            json_rpc_url: Url::from_str("http://127.0.0.1:8080").unwrap(),
            dev_api_url: Url::from_str("http://127.0.0.1:8080").unwrap(),
            faucet_url: None,
        }
    }
}

/// Generates the typescript bindings for the main Move package based on the embedded
/// diem types and Move stdlib. Mimics much of the transaction_builder_generator's CLI
/// except with typescript defaults and embedded content, as opposed to repo directory paths.
pub fn generate_typescript_libraries(project_path: &Path) -> Result<()> {
    let pkg_path = project_path.join(MAIN_PKG_PATH);
    let _compiled_package = build_move_package(&pkg_path)?;
    let target_dir = pkg_path.join("generated");
    let installer = serdegen::typescript::Installer::new(target_dir.clone());
    generate_runtime(&installer)?;
    generate_transaction_builders(&pkg_path, &target_dir)?;
    Ok(())
}

fn generate_runtime(installer: &serdegen::typescript::Installer) -> Result<()> {
    installer
        .install_serde_runtime()
        .map_err(|e| anyhow::anyhow!("unable to install Serde runtime: {:?}", e))?;
    installer
        .install_bcs_runtime()
        .map_err(|e| anyhow::anyhow!("unable to install BCS runtime: {:?}", e))?;

    // diem types
    let diem_types_content = String::from_utf8_lossy(include_bytes!(
        "../../../testsuite/generate-format/tests/staged/diem.yaml"
    ));
    let mut registry = serde_yaml::from_str::<Registry>(diem_types_content.as_ref())?;
    buildgen::typescript::replace_keywords(&mut registry);

    let config = serdegen::CodeGeneratorConfig::new("diemTypes".to_string())
        .with_encodings(vec![serdegen::Encoding::Bcs]);
    installer
        .install_module(&config, &registry)
        .map_err(|e| anyhow::anyhow!("unable to install typescript diem types: {:?}", e))?;
    Ok(())
}

/// Builds a package using the move package system.
pub fn build_move_package(pkg_path: &Path) -> Result<CompiledPackage> {
    println!("Building {}...", pkg_path.display());
    let config = move_package::BuildConfig {
        dev_mode: true,
        test_mode: false,
        generate_docs: false,
        generate_abis: true,
        force_recompilation: false,
        install_dir: None,
    };

    config.compile_package(pkg_path, &mut std::io::stdout())
}

fn generate_transaction_builders(pkg_path: &Path, target_dir: &Path) -> Result<()> {
    let module_name = "diemStdlib";
    let abi_directory = pkg_path;
    let abis = buildgen::read_abis(&[abi_directory])?;

    let installer: buildgen::typescript::Installer =
        buildgen::typescript::Installer::new(PathBuf::from(target_dir));
    installer
        .install_transaction_builders(module_name, abis.as_slice())
        .map_err(|e| anyhow::anyhow!("unable to install transaction builders: {:?}", e))?;
    Ok(())
}

pub fn normalized_project_path(project_path: Option<PathBuf>) -> Result<PathBuf> {
    match project_path {
        Some(path) => Ok(path),
        None => get_shuffle_project_path(&std::env::current_dir()?),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{new, shared::Home};
    use diem_crypto::PrivateKey;
    use diem_infallible::duration_since_epoch;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_get_shuffle_project_path() {
        let tmpdir = tempdir().unwrap();
        let dir_path = tmpdir.path();

        std::fs::create_dir_all(dir_path.join("nested")).unwrap();
        std::fs::write(dir_path.join("Shuffle.toml"), "goodday").unwrap();

        let actual = get_shuffle_project_path(dir_path.join("nested").as_path()).unwrap();
        let expectation = dir_path;
        assert_eq!(&actual, expectation);
    }

    #[test]
    fn test_network_home_create_archive_dir() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("localhost/accounts")).unwrap();

        let network_home = NetworkHome::new(dir.path().join("localhost").as_path());
        let time = duration_since_epoch();
        network_home.create_archive_dir(time).unwrap();
        let test_archive_dir = dir
            .path()
            .join("localhost/accounts")
            .join(time.as_secs().to_string());
        assert_eq!(test_archive_dir.is_dir(), true);
    }

    #[test]
    fn test_network_home_archive_old_key() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("localhost/accounts/latest")).unwrap();

        let network_home = NetworkHome::new(dir.path().join("localhost").as_path());
        let private_key = network_home.generate_key_file().unwrap();

        let time = duration_since_epoch();
        let archived_dir = network_home.create_archive_dir(time).unwrap();
        network_home.archive_old_key(&archived_dir).unwrap();
        let test_archive_key_path = dir
            .path()
            .join("localhost/accounts")
            .join(time.as_secs().to_string())
            .join("dev.key");
        let archived_key = generate_key::load_key(test_archive_key_path);

        assert_eq!(private_key, archived_key);
    }

    #[test]
    fn test_network_home_archive_old_address() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("localhost/accounts/latest")).unwrap();

        let network_home = NetworkHome::new(dir.path().join("localhost").as_path());
        let private_key = network_home.generate_key_file().unwrap();
        network_home
            .generate_address_file(&private_key.public_key())
            .unwrap();
        let address_path = dir.path().join("localhost/accounts/latest/address");

        let time = duration_since_epoch();
        let archived_dir = network_home.create_archive_dir(time).unwrap();
        network_home.archive_old_address(&archived_dir).unwrap();
        let test_archive_address_path = dir
            .path()
            .join("localhost/accounts")
            .join(time.as_secs().to_string())
            .join("address");

        let old_address = fs::read_to_string(address_path).unwrap();
        let archived_address = fs::read_to_string(test_archive_address_path).unwrap();

        assert_eq!(old_address, archived_address);
    }

    #[test]
    fn test_network_generate_network_accounts_path_if_nonexistent() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("localhost")).unwrap();

        let network_home = NetworkHome::new(dir.path().join("localhost").as_path());
        network_home
            .generate_network_accounts_path_if_nonexistent()
            .unwrap();
        assert_eq!(dir.path().join("localhost/accounts").is_dir(), true);
    }

    #[test]
    fn test_network_home_generate_network_latest_account_path_if_nonexistent() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("localhost/accounts")).unwrap();

        let network_home = NetworkHome::new(dir.path().join("localhost").as_path());
        network_home
            .generate_network_latest_account_path_if_nonexistent()
            .unwrap();
        assert_eq!(dir.path().join("localhost/accounts/latest").is_dir(), true);
    }

    #[test]
    fn test_network_home_generate_key_file() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("localhost/accounts/latest")).unwrap();

        let network_home = NetworkHome::new(dir.path().join("localhost").as_path());
        network_home.generate_key_file().unwrap();
        assert_eq!(
            dir.path()
                .join("localhost/accounts/latest/dev.key")
                .exists(),
            true
        );
    }

    #[test]
    fn test_network_home_generate_address_file() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("localhost/accounts/latest")).unwrap();

        let network_home = NetworkHome::new(dir.path().join("localhost").as_path());
        let public_key = network_home.generate_key_file().unwrap().public_key();
        network_home.generate_address_file(&public_key).unwrap();
        assert_eq!(
            dir.path()
                .join("localhost/accounts/latest/address")
                .exists(),
            true
        );
    }

    #[test]
    fn test_home_get_shuffle_path() {
        let dir = tempdir().unwrap();
        let home = Home::new(dir.path()).unwrap();
        let correct_dir = dir.path().join(".shuffle");
        assert_eq!(correct_dir, home.get_shuffle_path());
    }

    #[test]
    #[ignore]
    // Tests if the generated typesript libraries can actually be run by deno runtime.
    // `ignore`d tests are still run on CI via codegen-unit-test, but help keep
    // the local testsuite fast for devs.
    fn test_generate_typescript_libraries() {
        let tmpdir = tempdir().unwrap();
        let dir_path = tmpdir.path();
        new::write_example_move_packages(dir_path).expect("unable to create move main pkg");
        generate_typescript_libraries(dir_path).expect("unable to generate TS libraries");

        let script_path = dir_path.join("main/generated/diemStdlib/mod.ts");
        let output = std::process::Command::new("deno")
            .args(["run", script_path.to_string_lossy().as_ref()])
            .output()
            .unwrap();
        assert!(output.status.success());

        let script_contents = std::fs::read(script_path.to_string_lossy().as_ref()).unwrap();
        assert!(String::from_utf8_lossy(script_contents.as_ref())
            .contains("static encodeSetMessageScript(message: Uint8Array): DiemTypes.Script"));
    }

    #[test]
    fn test_network_home_get_latest_path() {
        let dir = tempdir().unwrap();
        let network_home = NetworkHome::new(dir.path().join("localhost").as_path());
        let correct_dir = dir.path().join("localhost/accounts/latest");
        assert_eq!(correct_dir, network_home.get_latest_account_path());
    }

    #[test]
    fn test_network_home_get_accounts_path() {
        let dir = tempdir().unwrap();
        let network_home = NetworkHome::new(dir.path().join("localhost").as_path());
        let correct_dir = dir.path().join("localhost/accounts");
        assert_eq!(correct_dir, network_home.get_accounts_path());
    }

    #[test]
    fn test_home_get_nodeconfig_path() {
        let dir = tempdir().unwrap();
        let home = Home::new(dir.path()).unwrap();
        let correct_dir = dir.path().join(".shuffle/nodeconfig/0/node.yaml");
        assert_eq!(correct_dir, home.get_validator_config_path());
    }

    #[test]
    fn test_home_generate_shuffle_path_if_nonexistent() {
        let dir = tempdir().unwrap();
        let home = Home::new(dir.path()).unwrap();
        assert_eq!(dir.path().join(".shuffle").exists(), false);
        home.generate_shuffle_path_if_nonexistent().unwrap();
        assert_eq!(dir.path().join(".shuffle").exists(), true);
    }

    #[test]
    fn test_home_get_root_key_path() {
        let dir = tempdir().unwrap();
        let home = Home::new(dir.path()).unwrap();
        let correct_dir = dir.path().join(".shuffle/nodeconfig/mint.key");
        assert_eq!(correct_dir, home.get_root_key_path());
    }

    #[test]
    fn test_network_home_get_latest_key_path() {
        let dir = tempdir().unwrap();
        let network_home = NetworkHome::new(dir.path().join("localhost").as_path());
        let correct_dir = dir.path().join("localhost/accounts/latest/dev.key");
        assert_eq!(correct_dir, network_home.get_latest_account_key_path());
    }

    #[test]
    fn test_network_home_save_root_key() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("localhost/accounts/latest")).unwrap();
        let user_root_key_path = dir.path().join("root.key");
        let user_root_key = generate_key::generate_and_save_key(&user_root_key_path);

        let network_home = NetworkHome::new(dir.path().join("localhost").as_path());
        network_home.save_root_key(&user_root_key_path).unwrap();
        let new_root_key = generate_key::load_key(network_home.get_latest_account_key_path());

        assert_eq!(new_root_key, user_root_key);
    }

    #[test]
    fn test_normalized_network() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".shuffle")).unwrap();
        let home = Home::new(dir.path()).unwrap();
        home.write_default_networks_config_into_toml_if_nonexistent()
            .unwrap();

        let url_from_some = normalized_network_url(&home, Some("localhost".to_string())).unwrap();
        let url_from_none = normalized_network_url(&home, None).unwrap();

        let correct_url = Url::from_str("http://127.0.0.1:8080").unwrap();

        assert_eq!(url_from_some, correct_url);
        assert_eq!(url_from_none, correct_url);
    }

    #[test]
    fn test_network_home_check_account_dir_exists() {
        let bad_dir = tempdir().unwrap();
        let network_home = NetworkHome::new(bad_dir.path().join("localhost").as_path());
        assert_eq!(network_home.check_account_path_exists().is_err(), true);

        let good_dir = tempdir().unwrap();
        fs::create_dir_all(good_dir.path().join("localhost/accounts")).unwrap();
        let network_home = NetworkHome::new(good_dir.path().join("localhost").as_path());
        assert_eq!(network_home.check_account_path_exists().is_err(), false);
    }

    #[test]
    fn test_read_networks_toml() {
        let dir = tempdir().unwrap();
        let home = Home::new(dir.path()).unwrap();
        fs::create_dir_all(dir.path().join(".shuffle")).unwrap();
        home.write_default_networks_config_into_toml_if_nonexistent()
            .unwrap();
        let networks_cfg = home.read_networks_toml().unwrap();
        assert_eq!(networks_cfg, NetworksConfig::default());
    }

    fn get_test_localhost_network() -> Network {
        Network {
            name: "localhost".to_string(),
            json_rpc_url: Url::from_str("http://127.0.0.1:8080").unwrap(),
            dev_api_url: Url::from_str("http://127.0.0.1:8080").unwrap(),
            faucet_url: None,
        }
    }

    fn get_test_fake_trove_testnet() -> Network {
        Network {
            name: "trove_testnet".to_string(),
            json_rpc_url: Url::from_str("http://fake_trove_json_rpc.net").unwrap(),
            dev_api_url: Url::from_str("http://fake_trove_dev_api.net").unwrap(),
            faucet_url: Some(Url::from_str("http://fake_faucet_dev_api.net").unwrap()),
        }
    }

    fn get_test_networks_config() -> NetworksConfig {
        let mut network_map = BTreeMap::new();
        network_map.insert("localhost".to_string(), get_test_localhost_network());
        NetworksConfig {
            networks: network_map,
        }
    }

    #[test]
    fn test_generate_default_networks_config() {
        assert_eq!(NetworksConfig::default(), get_test_networks_config());
    }

    #[test]
    fn test_networks_config_get() {
        let networks_config = get_test_networks_config();
        let network = networks_config.get("localhost").unwrap();
        let correct_network = get_test_localhost_network();
        assert_eq!(network, correct_network);
    }

    #[test]
    fn test_generate_default_network() {
        assert_eq!(Network::default(), get_test_localhost_network());
    }

    #[test]
    fn test_network_get_name() {
        let localhost_network = get_test_localhost_network();
        assert_eq!(localhost_network.get_name(), "localhost".to_string());

        let trove_testnet_network = get_test_fake_trove_testnet();
        assert_eq!(
            trove_testnet_network.get_name(),
            "trove_testnet".to_string()
        );
    }

    #[test]
    fn test_network_get_json_rpc_url() {
        let localhost_network = get_test_localhost_network();
        assert_eq!(
            localhost_network.get_json_rpc_url().unwrap(),
            Url::from_str("http://127.0.0.1:8080").unwrap()
        );

        let trove_testnet_network = get_test_fake_trove_testnet();
        assert_eq!(
            trove_testnet_network.get_json_rpc_url().unwrap(),
            Url::from_str("http://fake_trove_json_rpc.net").unwrap()
        );
    }

    #[test]
    fn test_network_get_dev_api_url() {
        let localhost_network = get_test_localhost_network();
        assert_eq!(
            localhost_network.get_dev_api_url().unwrap(),
            Url::from_str("http://127.0.0.1:8080").unwrap()
        );

        let trove_testnet_network = get_test_fake_trove_testnet();
        assert_eq!(
            trove_testnet_network.get_dev_api_url().unwrap(),
            Url::from_str("http://fake_trove_dev_api.net").unwrap()
        );
    }

    #[test]
    fn test_network_get_faucet_url() {
        let bad_network = get_test_localhost_network();
        assert_eq!(bad_network.get_faucet_url().is_err(), true);

        let good_network = get_test_fake_trove_testnet();
        assert_eq!(
            good_network.get_faucet_url().unwrap(),
            Url::from_str("http://fake_faucet_dev_api.net").unwrap()
        );
    }

    #[test]
    fn test_get_filtered_envs_for_deno() {
        let dir = tempdir().unwrap();
        let home = Home::new(dir.path()).unwrap();
        let project_path = Path::new("/Users/project_path");
        let network = get_test_localhost_network();
        let key_path = Path::new("/Users/private_key_path/dev.key");
        let address = AccountAddress::random();
        let filtered_envs =
            get_filtered_envs_for_deno(&home, project_path, &network, key_path, address).unwrap();

        assert_eq!(
            filtered_envs.get("PROJECT_PATH").unwrap(),
            &project_path.to_string_lossy().to_string()
        );
        assert_eq!(
            filtered_envs.get("SHUFFLE_BASE_NETWORKS_PATH").unwrap(),
            home.get_networks_path().to_string_lossy().as_ref(),
        );
        assert_eq!(
            filtered_envs.get("SENDER_ADDRESS").unwrap(),
            &address.to_hex_literal()
        );
        assert_eq!(
            filtered_envs.get("PRIVATE_KEY_PATH").unwrap(),
            &key_path.to_string_lossy().to_string()
        );
        assert_eq!(
            filtered_envs.get("SHUFFLE_NETWORK_NAME").unwrap(),
            &network.name
        );
        assert_eq!(
            filtered_envs.get("SHUFFLE_NETWORK_DEV_API_URL").unwrap(),
            &network.dev_api_url.to_string()
        )
    }

    #[test]
    fn test_parse_json_for_seq_num() {
        let value_obj = json!({
            "type":"0x1::DiemAccount::DiemAccount",
            "value": {
                "authentication_key": "0x88cae30f0fea7879708788df9e7c9b7524163afcc6e33b0a9473852e18327fa9",
                "key_rotation_capability":{
                    "vec":[{"account_address":"0x24163afcc6e33b0a9473852e18327fa9"}]
                },
                "received_events":{
                    "counter":"0",
                    "guid":{}
                },
                "sent_events":{},
                "sequence_number":"3",
                "withdraw_capability":{
                    "vec":[{"account_address":"0x24163afcc6e33b0a9473852e18327fa9"}]
                }
            }
        });

        let json_obj: Vec<Value> = vec![value_obj];
        let ret_seq_num = DevApiClient::parse_json_for_account_seq_num(json_obj).unwrap();
        assert_eq!(ret_seq_num, 3);
    }

    #[test]
    fn test_check_accounts_resources_response_status_code() {
        assert_eq!(
            DevApiClient::check_accounts_resources_response_status_code(
                &StatusCode::from_u16(200).unwrap()
            )
            .is_err(),
            false
        );
        assert_eq!(
            DevApiClient::check_accounts_resources_response_status_code(
                &StatusCode::from_u16(404).unwrap()
            )
            .is_err(),
            true
        );
    }

    #[test]
    fn test_home_check_networks_toml_exists() {
        let dir = tempdir().unwrap();
        let home = Home::new(dir.path()).unwrap();
        assert_eq!(home.check_networks_toml_exists().is_err(), true);
        fs::create_dir_all(dir.path().join(".shuffle")).unwrap();
        home.write_default_networks_config_into_toml_if_nonexistent()
            .unwrap();
        assert_eq!(home.check_networks_toml_exists().is_err(), false);
    }
}
