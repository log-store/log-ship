use std::env::home_dir;
use std::fs::{File};
use std::io::{BufReader, Read};

use std::path::PathBuf;


use anyhow::{anyhow, Context};

use serde::de::DeserializeOwned;


/// Looks in the following places for the config file
/// - current directory
/// - user's directory
/// - /etc/
/// If the file log-store.toml isn't found, returns the list of files checked
pub fn find_config_file(config_file_name: &str) -> Result<PathBuf, Vec<PathBuf>> {
    // start with the current directory
    let config_file_path = PathBuf::from(config_file_name);

    if config_file_path.exists() {
        return Ok(config_file_path)
    }

    let mut checked_paths = vec![config_file_path];

    // next try the user's directory
    if let Some(user_dir) = home_dir() {
        let config_file_path = user_dir.join(config_file_name);

        if config_file_path.exists() {
            return Ok(config_file_path)
        }

        checked_paths.push(config_file_path);
    }

    // finally look for it in /etc
    let config_file_path = PathBuf::from("/etc/").join(config_file_name);

    if config_file_path.exists() {
        Ok(config_file_path)
    } else {
        checked_paths.push(config_file_path);
        Err(checked_paths)
    }
}


/// Parse the config file
pub fn parse_config_file<T: DeserializeOwned>(config_file_path: PathBuf) -> anyhow::Result<T> {
    // open the config file
    let mut file = BufReader::new(File::open(&config_file_path)
        .context(format!("Error opening config file: {}", config_file_path.display()))
        .map_err(|e| anyhow!("Error opening config file: {:?}", e))?
    );
    let mut toml_str = String::new();

    // read the whole thing into a string
    file.read_to_string(&mut toml_str)
        .context(format!("Error reading config file: {}", config_file_path.display()))
        .map_err(|e| anyhow!("Error reading config file: {:?}", e))?;

    // parse into the ConfigFile struct
    let config_file: T = toml::from_str(toml_str.as_str())
        .context(format!("Failed parsing config file: {}", config_file_path.display()))
        .map_err(|e| anyhow!("parsing config file: {:?}", e))?;

    Ok(config_file)
}