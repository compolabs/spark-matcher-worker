use std::env;
use serde::Deserialize;

use crate::error::Error;

pub fn ev(key: &str) -> Result<String, Error> {
    env::var(key).map_err(|e| Error::EnvVarError(key.to_owned(), e.to_string()))
}

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub uuid: String,
    pub mnemonic: String,
    pub contract_id: String,
    pub websocket_url: String,
    pub chain: String,
}
