use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Fuel error: {0}")]
    FuelError(#[from] fuels::types::errors::Error),

    #[error("Failed to retrieve environment variable '{0}': {1}")]
    EnvVarError(String, String),

    #[error("Fuel_crypto private key parsing error")]
    FuelCryptoPrivParseError,

    #[error("Failed to match orders: {0}")]
    MatchOrdersError(String),

    #[error("Failed to parse order amount: {0}")]
    OrderAmountParseError(String),

    #[error("Failed to parse contract ID")]
    ContractIdParseError(#[from] std::num::ParseIntError),

    #[error("Failed to parse contract ID")]
    ProcessMessagePayloadError(String),

    #[error("String parsing error: {0}")]
    StringParsingError(String),

    #[error("Url parse error {0}")]
    UrlParseError(#[from] url::ParseError),

    #[error("Serde json error {0}")]
    SerdeJsonError(#[from] serde_json::Error),

    #[error("Tokio tungstenite error {0}")]
    TokioTungsteniteError(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("Tokio tungstenite stream error {0}")]
    TokioTungsteniteStreamError(#[from] std::io::Error),
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Error::StringParsingError(s.to_string())
    }
}
