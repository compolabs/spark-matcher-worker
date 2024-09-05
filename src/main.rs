mod matcher_client;
mod error;

use error::Error;
use matcher_client::MatcherClient;
use url::Url;

#[tokio::main]
async fn main() -> Result<(), Error>{
    env_logger::init();

    let ws_url = Url::parse("ws://localhost:9001").expect("Invalid WebSocket URL");
    let matcher_client = MatcherClient::new(ws_url);

    match matcher_client.connect().await {
        Ok(_) => println!("Matcher connected successfully"),
        Err(e) => eprintln!("Failed to connect matcher: {:?}", e),
    }
    Ok(())
}
