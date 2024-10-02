use fuels::{accounts::{provider::Provider, wallet::WalletUnlocked}, types::ContractId};
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message, WebSocketStream};
use url::Url;
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;
use uuid::Uuid;
use schemars::JsonSchema;
use std::sync::Mutex;

use crate::{error::Error, config::Settings, order_processor::OrderProcessor};

#[derive(Debug, PartialEq, Eq, Clone, Copy, JsonSchema, Serialize, Deserialize)]
pub enum OrderType {
    Buy,
    Sell,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, JsonSchema, Serialize, Deserialize)]
pub enum OrderStatus {
    New,
    InProgress,
    PartiallyFilled,
    Filled,
    Cancelled,
    Failed,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MatcherOrderUpdate {
    pub order_id: String,
    pub price: u128,
    pub timestamp: u64,
    pub new_amount: u128,
    pub status: Option<OrderStatus>,
    pub order_type: OrderType,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize)]
pub struct SpotOrder {
    pub id: String,
    pub user: String,
    pub asset: String,
    pub amount: u128,
    pub price: u128,
    pub timestamp: u64,
    pub order_type: OrderType,
    pub status: Option<OrderStatus>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MatcherConnectRequest {
    pub uuid: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MatcherRequest {
    pub orders: Vec<SpotOrder>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MatcherResponse {
    pub success: bool,
    pub orders: Vec<MatcherOrderUpdate>,  
}

pub struct MatcherClient {
    uuid: Uuid,
    ws_url: Url,
    settings: Arc<Settings>,
    hd_wallet_number: Arc<Mutex<u32>>,
    order_processor: OrderProcessor,
}

impl MatcherClient {
    pub fn new(ws_url: Url, settings: Arc<Settings>) -> Self {
        let uuid = Uuid::parse_str(&settings.uuid).expect("Invalid UUID format in configuration");

        MatcherClient {
            uuid,
            ws_url,
            settings: settings.clone(),
            hd_wallet_number: Arc::new(Mutex::new(0)),
            order_processor: OrderProcessor::new(settings.clone()),
        }
    }

    pub async fn connect(&self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            match self.connect_to_ws().await {
                Ok(_) => info!("Matcher {} connected successfully!", self.uuid),
                Err(e) => error!("Failed to connect Matcher {}: {:?}", self.uuid, e),
            }
            sleep(Duration::from_secs(20)).await;
        }
    }

    async fn connect_to_ws(&self) -> Result<(), Box<dyn std::error::Error>> {
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
                            if let Ok(request) = serde_json::from_str::<MatcherRequest>(&text) {
                                self.process_orders(request.orders, &mut write).await?;
                            }
                        }
                        Ok(Message::Close(_)) => {
                            info!("Connection closed by server. Reconnecting...");
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
                return Err(Box::new(Error::ConnectionError(e.to_string())));
            }
        }
        Ok(())
    }

    async fn process_orders(
        &self,
        orders: Vec<SpotOrder>,
        write: &mut futures_util::stream::SplitSink<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Message>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        
        let result = self.order_processor.match_orders(orders.clone()).await;

        let response = match result {
            Ok(updates) => MatcherResponse {
                success: true,
                orders: updates,  
            },
            Err(_) => {
                let failed_updates: Vec<MatcherOrderUpdate> = orders.into_iter().map(|order| MatcherOrderUpdate {
                    order_id: order.id,
                    price: order.price,
                    timestamp: order.timestamp,
                    new_amount: order.amount, 
                    status: Some(OrderStatus::Failed), 
                    order_type: order.order_type,
                }).collect();

                MatcherResponse {
                    success: false,
                    orders: failed_updates,  
                }
            }
        };

        let response_msg = serde_json::to_string(&response)?;
        write.send(Message::Text(response_msg)).await?;
        info!("Sent match result for orders");

        sleep(Duration::from_millis(500)).await;

        Ok(())
    }
}
