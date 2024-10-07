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
pub enum MatcherRequest {
    Orders(Vec<SpotOrder>),
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
                            if let Ok(MatcherRequest::Orders(orders)) = serde_json::from_str::<MatcherRequest>(&text) {
                                self.process_orders(orders, &mut write).await?;
                            } else {
                                error!("Failed to parse MatcherRequest");
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
                        _ => {
                            error!("Unexpected message format");
                        }
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
        info!("Processing batch of {} orders", orders.len()); 

        
        let (min_sell, max_buy) = self.calculate_spread(&orders);
        info!("Batch spread - Max Buy: {}, Min Sell: {}", max_buy.unwrap_or(0), min_sell.unwrap_or(0));

        // Логика правильного спреда: если Max Buy >= Min Sell, это хороший спред
        if let (Some(max_buy), Some(min_sell)) = (max_buy, min_sell) {
            if max_buy < min_sell {
                error!("Invalid batch: Max Buy ({}) < Min Sell ({}), batch rejected", max_buy, min_sell);
                return Err(Box::new(Error::InvalidBatchError)); // Ошибка, если Max Buy меньше Min Sell
            }
        }

        info!("Orders:");
        for o in &orders {
            info!("id: {:?} | status {:?}, | type {:?}", o.id, o.status, o.order_type);
        }
        let order_ids : Vec<String> = orders.clone().into_iter().map(|o|o.id).collect();
        info!("orders {:?}", order_ids);

        let result = self.order_processor.match_orders(orders.clone()).await;

        let response = match result {
            Ok(updates) => {
                info!("Orders processed successfully");  
                MatcherResponse {
                    success: true,
                    orders: updates,
                }
            }
            Err(err) => {
                error!("Error while processing orders: {:?}", err);  
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
        if let Err(e) = write.send(Message::Text(response_msg)).await {
            error!("Failed to send response to middleware: {:?}", e);
            return Err(Box::new(Error::SendResponseError(e.to_string())));
        }
        info!("Sent match result for orders");

        sleep(Duration::from_millis(500)).await;

        Ok(())
    }

    fn calculate_spread(&self, orders: &Vec<SpotOrder>) -> (Option<u128>, Option<u128>) {
        let mut min_sell: Option<u128> = None;
        let mut max_buy: Option<u128> = None;

        for order in orders {
            match order.order_type {
                OrderType::Buy => {
                    if let Some(max) = max_buy {
                        if order.price > max {
                            max_buy = Some(order.price);
                        }
                    } else {
                        max_buy = Some(order.price);
                    }
                }
                OrderType::Sell => {
                    if let Some(min) = min_sell {
                        if order.price < min {
                            min_sell = Some(order.price);
                        }
                    } else {
                        min_sell = Some(order.price);
                    }
                }
            }
        }

        // Возвращаем значения в правильном порядке: сначала max_buy, затем min_sell
        (max_buy, min_sell)
    }
}
