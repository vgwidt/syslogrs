use anyhow::{Context, Result};
use serde_derive::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
pub struct Data {
    config: Config,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    pub display_stdout: bool,
    pub store_logs: bool,
    pub compress: bool,
    pub port: u16,
    pub log_rotate_interval: u64,
    pub log_dir: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            display_stdout: true,
            store_logs: false,
            compress: false,
            port: 514,
            log_rotate_interval: 86400,
            log_dir: "./".to_string(),
        }
    }
}

pub fn get_config(config_paths: Vec<&str>) -> Result<Config> {
    // iterate through vec to find first valid config path
    // find will return none if none are found
    let config_path: Option<&str> = config_paths
        .iter()
        .find(|&file| Path::new(file).exists())
        .cloned();

    if let Some(path) = config_path {
        let contents = std::fs::read_to_string(path).context("Failed to read config file")?;
        let data: Data =
            toml::from_str(&contents).context(format!("Bad toml data from file: {}", path))?;
        Ok(data.config)
    } else {
        println!("No config file found, using defaults");
        Ok(Config::default())
    }
}
