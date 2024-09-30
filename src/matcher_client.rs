use fuels::{accounts::{provider::Provider, wallet::WalletUnlocked}, types::ContractId};
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use serde::{Deserialize, Serialize};
use spark_market_sdk::SparkMarketContract;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message, WebSocketStream};
use url::Url;
use std::{collections::HashSet, str::FromStr, time::Duration};
use tokio::time::sleep;
use uuid::Uuid;
use std::sync::Arc;

use crate::error::Error;
use crate::config::Settings;

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
    pub settings: Arc<Settings>,
    pub hd_wallet_number: Arc<std::sync::Mutex<u32>>, 
}

impl MatcherClient {
    pub fn new(ws_url: Url, settings: Arc<Settings>) -> Self {
        let uuid = Uuid::parse_str(&settings.uuid)
            .expect("Invalid UUID format in configuration");

        MatcherClient {
            uuid,
            ws_url,
            settings,
            hd_wallet_number: Arc::new(std::sync::Mutex::new(0)), 
        }
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
            sleep(Duration::from_secs(20)).await;
        }
    }

    async fn connect_to_ws(
        &self,
    ) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            match connect_async(&self.ws_url).await {
                Ok((ws_stream, _)) => {
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
                                if let Ok(MatcherRequest::Orders(orders)) = serde_json::from_str::<MatcherRequest>(&text) {
                                    let match_result = self.match_orders(orders.clone()).await;

                                    let response = match match_result {
                                        Ok(_) => MatcherMessage::MatchResult {
                                            success: true,
                                            orders: orders.into_iter().map(|order| order.id).collect(),
                                        },
                                        Err(_) => MatcherMessage::MatchResult {
                                            success: false,
                                            orders: orders.into_iter().map(|order| order.id).collect(),
                                        },
                                    };

                                    let response_msg = serde_json::to_string(&response)?;
                                    write.send(Message::Text(response_msg)).await?;
                                    info!("Sent match result for orders");

                                    
                                    sleep(Duration::from_millis(500)).await;
                                }
                            }
                            Ok(Message::Close(_)) => {
                                info!("Connection closed by server. Attempting to reconnect...");
                                break;
                            }
                            Err(e) => {
                                error!("WebSocket error: {:?}", e);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to connect: {:?}", e);
                }
            }

            info!("Retrying connection in 20 seconds...");
            sleep(Duration::from_secs(20)).await;
        }
    }

    
    pub async fn match_orders(&self, orders: Vec<SpotOrder>) -> Result<(), Error> {
        let provider = Provider::connect("testnet.fuel.network").await?;

        
        let mut hd_wallet_number = self.hd_wallet_number.lock().unwrap();
        let current_wallet_number = *hd_wallet_number;
        *hd_wallet_number = (*hd_wallet_number + 1) % 10; 

        let path = format!("m/44'/1179993420'/{}'/0/0", current_wallet_number);
        let wallet = WalletUnlocked::new_from_mnemonic_phrase_with_path(
            &self.settings.mnemonic,
            Some(provider.clone()),
            &path,
        ).expect("Failed to create wallet");

        let market = SparkMarketContract::new(ContractId::from_str(&self.settings.contract_id)?, wallet).await;

        let unique_order_ids: HashSet<String> = orders.into_iter().map(|order| order.id).collect();
        info!("Processing {} orders with HD wallet {}", unique_order_ids.len(), current_wallet_number);

        let unique_bits256_ids: Vec<fuels::types::Bits256> = unique_order_ids
            .iter()
            .map(|id| fuels::types::Bits256::from_hex_str(id).unwrap())
            .collect();

        info!("Attempting to match orders...");

        match market.match_order_many(unique_bits256_ids).await {
            Ok(result) => {
                info!(
                    "Matched orders successfully with HD wallet {}. Transaction ID: {:?}, Gas used: {:?}",
                    current_wallet_number, result.tx_id, result.gas_used
                );
                Ok(())
            }
            Err(e) => {
                error!("Error while matching orders with HD wallet {}: {:?}", current_wallet_number, e);
                Err(Error::MatchOrdersError(e.to_string()))
            }
        }
    }
}
