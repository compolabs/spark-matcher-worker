use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream};
use url::Url;
use std::time::Duration;
use tokio::time::sleep;
use dotenv::dotenv;
use std::env;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct MatcherConnectRequest {
    pub uuid: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum MatcherMessage {
    Identify { uuid: String },
    MatchResult { success: bool, orders: Vec<String> },
}

pub struct MatcherClient {
    pub uuid: Uuid,
    pub ws_url: Url,
}

impl MatcherClient {
    pub fn new(ws_url: Url) -> Self {
        dotenv().ok(); 

        let uuid = match env::var("MATCHER_UUID") {
            Ok(uuid_str) => Uuid::parse_str(&uuid_str).expect("Invalid UUID format in .env"),
            Err(_) => {
                let new_uuid = Uuid::new_v4();
                println!("No MATCHER_UUID found in .env. Generated new UUID: {}", new_uuid);
                new_uuid
            }
        };

        MatcherClient { uuid, ws_url }
    }

    pub async fn connect(&self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            match self.connect_to_ws().await {
                Ok(_) => {
                    info!("Matcher {} connected successfully!", self.uuid);
                }
                Err(e) => {
                    error!("Failed to connect Matcher {}: {:?}", self.uuid, e);
                }
            }
            sleep(Duration::from_secs(10)).await; 
        }
    }

    async fn connect_to_ws(
        &self,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (ws_stream, _) = connect_async(&self.ws_url).await?;
        let (mut write, mut read) = ws_stream.split();

        let identify_message = MatcherConnectRequest {
            uuid: self.uuid.to_string(),
        };
        let msg = serde_json::to_string(&identify_message)?;
        info!("Sending identification message: {}", msg);
        write.send(Message::Text(msg)).await?;

        info!("Sent identification message for matcher: {}", self.uuid);

        while let Some(message) = read.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    info!("Received message: {}", text);

                }
                Ok(Message::Close(_)) => {
                    info!("Connection closed by server");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {:?}", e);
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }
}
