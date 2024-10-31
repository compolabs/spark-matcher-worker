mod config;
mod error;
mod matcher_client;
mod order_processor;
mod types;

use std::sync::Arc;
use dotenv::dotenv;

use config::{ev, Settings};
use matcher_client::MatcherClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    dotenv().ok();

    let settings = Settings {
        uuid: ev("UUID")?,
        mnemonic: ev("MNEMONIC")?,
        contract_id: ev("CONTRACT_ID")?,
        websocket_url: ev("WEBSOCKET_URL")?, 
        chain: ev("CHAIN")?,
    };
    let matcher_client = MatcherClient::new(Arc::new(settings));

    match matcher_client.connect().await {
        Ok(_) => println!("Matcher connected successfully"),
        Err(e) => eprintln!("Failed to connect matcher: {:?}", e),
    }
    Ok(())
}
