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
    pub number: i64,
}

impl AppConfig {
    pub fn new(config_path: &str) -> Self {
        let config_content = fs::read_to_string(config_path).expect("Failed to read config file");
        println!("config : {:?}", config_content);
        toml::from_str(&config_content).expect("Failed to parse config file")
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new("config.toml") 
    }
}
