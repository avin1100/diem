use shuffle::shared::{Home, NetworkHome, LOCALHOST_NAME};
use tempfile::{tempdir, TempDir};
use std::fs;
use diem_crypto::PrivateKey;
use serde_json;
use std::process::Output;

const HOME_STRUCT_HOME_PATH : &str = "--home-struct-home-path";
const BINARY: &str = env!("CARGO_BIN_EXE_shuffle");

fn create_project_and_account(home: &Home, network_home: &NetworkHome, dir: &TempDir) {
    fs::create_dir_all(dir.path().join(".shuffle/networks/localhost/accounts/latest")).unwrap();
    home.write_default_networks_config_into_toml_if_nonexistent().unwrap();

    let new_account_key =  network_home.generate_key_file().unwrap();
    let public_key = new_account_key.public_key();
    network_home.generate_latest_address_file(&public_key).unwrap();
}

fn shuffle_new(base_dir_string: &str, project_path_string: &str) {
    let mut shuffle_new_command = std::process::Command::new(BINARY);
    shuffle_new_command.args([HOME_STRUCT_HOME_PATH, base_dir_string ,"new", project_path_string]);
    shuffle_new_command.output().unwrap();
}

fn shuffle_node(base_dir_string: &str) {
    let mut shuffle_node_command = std::process::Command::new(BINARY);
    shuffle_node_command.args([HOME_STRUCT_HOME_PATH, base_dir_string ,"node"]);
    shuffle_node_command
        .spawn()
        .unwrap()
        .wait()
        .unwrap();
}

fn shuffle_deploy(base_dir_string: &str, project_path_string: &str) {
    let mut shuffle_deploy_command = std::process::Command::new(BINARY);
    shuffle_deploy_command.args([HOME_STRUCT_HOME_PATH, base_dir_string , "deploy", "-p", project_path_string]);
    shuffle_deploy_command.spawn().unwrap().wait().unwrap();
}

fn shuffle_transactions(base_dir_string: &str, additional_args: Option<Vec<&str>>) -> Output {
    let mut command = std::process::Command::new(BINARY);
    let basic_args = [HOME_STRUCT_HOME_PATH, base_dir_string ,"transactions"];
    let mut full_arg_list = Vec::from(basic_args);
    match additional_args {
        Some(mut arg) => full_arg_list.append(&mut arg),
        None => ()
    }

    command.args(full_arg_list).output().unwrap()
}

#[test]
fn test_transactions_without_making_a_project() {
    let base_dir_string = tempdir().unwrap().path().to_string_lossy().to_string();
    let output = shuffle_transactions(base_dir_string.as_str(), None);
    let std_err = String::from_utf8(output.stderr).unwrap();
    assert_eq!(std_err.contains("Error: A project hasn't been created yet. Run shuffle new first"), true);
    //todo add assert for exit codes for EVERYTHING
}

#[test]
fn test_transactions_after_making_a_project_without_account() {
    let dir = tempdir().unwrap();
    let home = Home::new(dir.path()).unwrap();

    fs::create_dir_all(dir.path().join(".shuffle")).unwrap();
    home.write_default_networks_config_into_toml_if_nonexistent().unwrap();

    let base_dir_string = dir.path().to_string_lossy().to_string();
    let output = shuffle_transactions(base_dir_string.as_str(), None);
    let std_err = String::from_utf8(output.stderr).unwrap();
    assert_eq!(std_err.contains("Error: An account hasn't been created yet! Run shuffle account first"), true);
}

#[test]
fn test_transactions_after_making_a_project_and_account() {
    let dir = tempdir().unwrap();
    let home = Home::new(dir.path()).unwrap();
    let network_home = home.new_network_home(LOCALHOST_NAME);

    create_project_and_account(&home, &network_home, &dir);

    let base_dir_string = dir.path().to_string_lossy().to_string();
    let output = shuffle_transactions(base_dir_string.as_str(), None);

    assert_eq!(output.status.code().unwrap(), 0);
}

// #[test]
// fn test_transactions_after_deploy() {
//     let base_dir = tempdir().unwrap();
//     let project_path = tempdir().unwrap();
//
//     let home = Home::new(base_dir.path()).unwrap();
//     let network_home = home.new_network_home(LOCALHOST_NAME);
//
//     create_project_and_account(&home, &network_home, &base_dir);
//     let base_dir_string = base_dir.path().to_string_lossy().to_string();
//     let project_path_string = project_path.path().to_string_lossy().to_string();
//
//     shuffle_new(base_dir_string.as_str(), project_path_string.as_str());
//     shuffle_node(base_dir_string.as_str());
//     shuffle_deploy(base_dir_string.as_str(), project_path_string.as_str());
//     let output = shuffle_transactions(base_dir_string.as_str(), None);
//     let raw_output = output.stdout;
//
//     let raw_output_string = String::from_utf8(raw_output).unwrap();
//
//     println!("RAW IS {}", raw_output_string);
//     let corrected_string = serde_json::to_string_pretty(raw_output_string.as_str()).unwrap();
//     println!("CORRECT IS {}", corrected_string.trim());
//
//     let json :serde_json::Value = serde_json::from_str(corrected_string.as_str()).unwrap();
//     println!("JSON IS {:?}", &json["type"]);
// }


#[test]
fn test_transactions_raw() {
    let base_dir = tempdir().unwrap();
    let project_path = tempdir().unwrap();

    let home = Home::new(base_dir.path()).unwrap();
    let network_home = home.new_network_home(LOCALHOST_NAME);

    create_project_and_account(&home, &network_home, &base_dir);
    let base_dir_string = base_dir.path().to_string_lossy().to_string();
    let project_path_string = project_path.path().to_string_lossy().to_string();

    shuffle_new(base_dir_string.as_str(), project_path_string.as_str());
    shuffle_node(base_dir_string.as_str());
    shuffle_deploy(base_dir_string.as_str(), project_path_string.as_str());

    let additional_args = vec!["--raw"];
    let output = shuffle_transactions(base_dir_string.as_str(), Some(additional_args));
    let raw_output = output.stdout;

    let raw_output_string = String::from_utf8(raw_output).unwrap();

    println!("RAW IS {}", raw_output_string);
    // let corrected_string = serde_json::to_string_pretty(raw_output_string.as_str()).unwrap();
    // println!("CORRECT IS {}", corrected_string.trim());

    let json :serde_json::Value = serde_json::from_str(raw_output_string.as_str()).unwrap();
    println!("JSON IS {:?}", &json["type"]);
}




