mod config;
mod error;
mod matcher_client;
mod order_processor;
mod types;

use dotenv::dotenv;
use std::sync::Arc;

use config::{ev, Settings};
use matcher_client::MatcherClient;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();
    dotenv().ok();

    let settings = Settings {
        uuid: ev("UUID")?,
        mnemonic: ev("MNEMONIC")?,
        contract_id: ev("CONTRACT_ID")?,
        websocket_url: ev("WEBSOCKET_URL")?,
        chain: ev("CHAIN")?,
    };
    info!(
        uuid = %settings.uuid,
        domain = %settings.websocket_url,
        chain = %settings.chain,
        contract_id = %settings.contract_id,
        "Starting spark-matcher-worker..."
    );
    let matcher_client = MatcherClient::new(Arc::new(settings));

    match matcher_client.connect().await {
        Ok(_) => info!("Matcher connected successfully"),
        Err(e) => error!("Failed to connect matcher: {:?}", e),
    }
    Ok(())
}
