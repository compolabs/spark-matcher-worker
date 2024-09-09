use fuels::{accounts::{provider::Provider, wallet::WalletUnlocked}, types::ContractId};
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use serde::{Deserialize, Serialize};
use spark_market_sdk::SparkMarketContract;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream};
use url::Url;
use std::{collections::HashSet, str::FromStr, time::Duration};
use tokio::time::sleep;
use dotenv::dotenv;
use std::env;
use uuid::Uuid;

use crate::error::Error;

#[derive(Serialize, Deserialize)]
pub enum MatcherRequest {
    Orders(Vec<SpotOrder>), 
}

#[derive(Serialize, Deserialize)]
pub enum MatcherResponse {
    MatchResult { success: bool, matched_orders: Vec<String> }, 
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotOrder {
    pub id: String,
    pub price: u128,
    pub order_type: String,  
}

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

                        if let Ok(MatcherRequest::Orders(orders)) = serde_json::from_str::<MatcherRequest>(&text) {
                            info!("Received orders for matching: {:?}", orders);

                            let match_result = match_orders(orders.clone()).await;

                            let response = match match_result {
                                Ok(_) => MatcherMessage::MatchResult { 
                                    success: true, 
                                    orders: orders.into_iter().map(|order| order.id).collect(), 
                                },
                                Err(_) => MatcherMessage::MatchResult { 
                                    success: false, 
                                    orders: orders.into_iter().map(|order| order.id).collect(), 
                                }
                            };

                            let response_msg = serde_json::to_string(&response)?;
                            write.send(Message::Text(response_msg)).await?;
                            info!("Sent match result for orders");
                        }
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

    pub async fn match_orders(orders: Vec<SpotOrder>) -> Result<(), Error> {
        let provider = Provider::connect("testnet.fuel.network").await?;
        
        let mnemonic = std::env::var("MNEMONIC").expect("MNEMONIC not set in env");
        let contract_id = std::env::var("CONTRACT_ID").expect("CONTRACT_ID not set in env");

        let wallet = WalletUnlocked::new_from_mnemonic_phrase(&mnemonic, Some(provider.clone()))
            .expect("Failed to create wallet");
        let market = SparkMarketContract::new(ContractId::from_str(&contract_id)? ,wallet).await;

        let unique_order_ids: HashSet<String> = orders.into_iter().map(|order| order.id).collect();
        let unique_bits256_ids: Vec<fuels::types::Bits256> = unique_order_ids
            .iter()
            .map(|id| fuels::types::Bits256::from_hex_str(id).unwrap())
            .collect();

        info!("Attempting to match orders...");
        match market.match_order_many(unique_bits256_ids).await {
            Ok(result) => {
                info!(
                    "Matched orders successfully. Transaction ID: {:?}, Gas used: {:?}",
                    result.tx_id, result.gas_used
                );
                Ok(())
            }
            Err(e) => {
                error!("Error while matching orders: {:?}", e);
                Err(Error::MatchOrdersError(e.to_string()))
            }
        }
}
