use serde::Deserialize;
use std::fs;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub settings: Settings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub uuid: String,
    pub mnemonic: String,
    pub contract_id: String,
    pub websocket_url: String,
}

impl AppConfig {
    pub fn new(config_path: &str) -> Self {
        let config_content = fs::read_to_string(config_path).expect("Failed to read config file");
        toml::from_str(&config_content).expect("Failed to parse config file")
    }
}
