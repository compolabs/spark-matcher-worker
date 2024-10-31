use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Fuel error: {0}")]
    Fuel(#[from] fuels::types::errors::Error),

    #[error("Failed to retrieve environment variable '{0}': {1}")]
    EnvVarError(String, String),

    #[error("Failed to match orders: {0}")]
    MatchOrders(String),

    #[error("Failed to parse contract ID")]
    ContractIdParse(#[from] std::num::ParseIntError),

    #[error("String parsing error: {0}")]
    StringParsing(String),

    #[error("Url parse error {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("Serde json error {0}")]
    SerdeJson(#[from] serde_json::Error),

    #[error("Tokio tungstenite error {0}")]
    TokioTungstenite(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("Tokio tungstenite stream error {0}")]
    TokioTungsteniteStream(#[from] std::io::Error),

    #[error("Invalid batch error")]
    InvalidBatch,
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Error::StringParsing(s.to_string())
    }
}
