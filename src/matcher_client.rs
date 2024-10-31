use crate::config::Settings;
use crate::order_processor::OrderProcessor;
use crate::types::{
    MatcherBatchRequest, MatcherConnectRequest, MatcherRequest, MatcherResponse, SpotOrder,
};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::sleep;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use uuid::Uuid;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MatcherClient {
    uuid: Uuid,
    settings: Arc<Settings>,
    order_processor: OrderProcessor,
    free_wallets: Arc<Semaphore>,
}

impl MatcherClient {
    pub fn new(settings: Arc<Settings>) -> Self {
        let uuid = Uuid::parse_str(&settings.uuid).expect("Invalid UUID format in configuration");

        MatcherClient {
            uuid,
            settings: settings.clone(),
            order_processor: OrderProcessor::new(settings.clone()),
            free_wallets: Arc::new(Semaphore::new(10)),
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
        let (ws_stream, _) = connect_async(&self.settings.websocket_url).await?;
        let (write, read) = ws_stream.split();
        let write = Arc::new(Mutex::new(write));

        self.send_identification_message(write.clone()).await?;

        self.handle_messages(write, read).await
    }

    async fn send_identification_message(
        &self,
        write: Arc<
            Mutex<
                futures_util::stream::SplitSink<
                    WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
                    Message,
                >,
            >,
        >,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let identify_message = MatcherRequest::Connect(MatcherConnectRequest {
            uuid: self.uuid.to_string(),
        });
        let msg = serde_json::to_string(&identify_message)?;
        info!("Sending identification message: {}", msg);

        let mut write_lock = write.lock().await;
        write_lock.send(Message::Text(msg)).await?;
        info!("Sent identification message for matcher: {}", self.uuid);

        Ok(())
    }

    async fn handle_messages(
        &self,
        write: Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>,
        mut read: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let free_wallets = self.free_wallets.clone();
        let order_processor = self.order_processor.clone();

        loop {
            // Попытка получить разрешение семафора
            let permit = match free_wallets.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    // Нет доступных кошельков, ждем
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };

            // Запрашиваем батч
            self.request_batch(write.clone()).await?;

            if let Some(message) = read.next().await {
                // Клонируем self
                let self_clone = self.clone();
                let write_clone = write.clone();
                let order_processor_clone = order_processor.clone();

                tokio::spawn(async move {
                    if let Err(e) = self_clone
                        .process_message(message, write_clone, order_processor_clone, permit)
                        .await
                    {
                        error!("Error processing message: {:?}", e);
                    }
                });
            } else {
                error!("No response from server");
                sleep(Duration::from_secs(5)).await;
            }

            sleep(Duration::from_secs(1)).await;
        }
    }

    async fn process_message(
        &self,
        message: Result<Message, tokio_tungstenite::tungstenite::Error>,
        write: Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>,
        order_processor: OrderProcessor,
        _permit: OwnedSemaphorePermit,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match message {
            Ok(Message::Text(text)) => match serde_json::from_str::<MatcherResponse>(&text) {
                Ok(MatcherResponse::Batch(orders)) if orders.is_empty() => {
                    info!("No orders available for matching. Waiting 10 seconds.");
                    sleep(Duration::from_secs(10)).await;
                }
                Ok(MatcherResponse::Batch(orders)) => {
                    self.process_orders(orders, write, order_processor).await;
                }
                Ok(MatcherResponse::NoOrders) => {
                    info!("Received NoOrders response. Waiting 1 seconds before retrying.");
                    sleep(Duration::from_secs(1)).await;
                }
                Ok(_) => {
                    info!("Received unknown response. Waiting 1 seconds before retrying.");
                    sleep(Duration::from_secs(1)).await;
                }
                Err(_) => {
                    error!("Failed to parse MatcherResponse: {}", text);
                }
            },
            Ok(Message::Close(_)) => {
                info!("Connection closed by server. Reconnecting...");
                return Ok(());
            }
            Err(e) => {
                error!("WebSocket error: {:?}", e);
                return Err(Box::new(e));
            }
            _ => {
                error!("Unexpected message format");
            }
        }
        Ok(())
    }

    async fn process_orders(
        &self,
        orders: Vec<SpotOrder>,
        write: Arc<
            Mutex<
                futures_util::stream::SplitSink<
                    WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
                    Message,
                >,
            >,
        >,
        order_processor: OrderProcessor,
    ) {
        let updates = match order_processor.process_orders(orders).await {
            Ok(updates) => updates,
            Err(e) => {
                error!("Error processing orders: {:?}", e);
                Vec::new()
            }
        };

        let response = MatcherRequest::OrderUpdates(updates);
        let response_msg = serde_json::to_string(&response).unwrap();

        let mut write_lock = write.lock().await;
        if let Err(e) = write_lock.send(Message::Text(response_msg)).await {
            error!("Failed to send updates to server: {:?}", e);
        }
    }

    async fn request_batch(
        &self,
        write: Arc<
            Mutex<
                futures_util::stream::SplitSink<
                    WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
                    Message,
                >,
            >,
        >,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let batch_request = MatcherBatchRequest {
            uuid: self.uuid.to_string(),
        };
        let request_msg = serde_json::to_string(&MatcherRequest::BatchRequest(batch_request))?;
        let mut write_lock = write.lock().await;
        write_lock.send(Message::Text(request_msg)).await?;
        info!("Requested batch from server");
        Ok(())
    }
}
