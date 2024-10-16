use crate::config::Settings;
use crate::error::Error;
use crate::order_processor::OrderProcessor;
use crate::types::{
    MatcherBatchRequest, MatcherConnectRequest, MatcherRequest, MatcherResponse, SpotOrder,
};
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::Message,
    MaybeTlsStream, WebSocketStream,
};
use url::Url;
use uuid::Uuid;

#[derive(Clone)]
pub struct MatcherClient {
    uuid: Uuid,
    ws_url: Url,
    settings: Arc<Settings>,
    order_processor: OrderProcessor,
    free_wallets: Arc<Mutex<u32>>, 
}

impl MatcherClient {
    pub fn new(ws_url: Url, settings: Arc<Settings>) -> Self {
        let uuid =
            Uuid::parse_str(&settings.uuid).expect("Invalid UUID format in configuration");

        MatcherClient {
            uuid,
            ws_url,
            settings: settings.clone(),
            order_processor: OrderProcessor::new(settings.clone()),
            free_wallets: Arc::new(Mutex::new(10)), 
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
        let (ws_stream, _) = connect_async(&self.ws_url).await?;
        let (write, read) = ws_stream.split();
        let write = Arc::new(Mutex::new(write));

        self.send_identification_message(write.clone()).await?;

        
        self.handle_messages(write, read).await
    }

    async fn send_identification_message(
        &self,
        write: Arc<Mutex<futures_util::stream::SplitSink<
            WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
            Message,
        >>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let identify_message = MatcherConnectRequest {
            uuid: self.uuid.to_string(),
        };
        let msg = serde_json::to_string(&identify_message)?;
        info!("Sending identification message: {}", msg);

        let mut write_lock = write.lock().await;
        write_lock.send(Message::Text(msg)).await?;
        info!("Sent identification message for matcher: {}", self.uuid);

        Ok(())
    }

    async fn handle_messages(
        &self,
        write: Arc<Mutex<futures_util::stream::SplitSink<
            WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
            Message,
        >>>,
        mut read: futures_util::stream::SplitStream<
            WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
        >,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let free_wallets = self.free_wallets.clone();
        let order_processor = self.order_processor.clone();

        loop {
            let wallets_available = self.check_wallets_availability().await;

            if wallets_available {
                self.request_batch(write.clone()).await?;

                if let Some(message) = read.next().await {
                    self.process_message(message, write.clone(), order_processor.clone(), free_wallets.clone()).await?;
                } else {
                    error!("No response from server");
                    sleep(Duration::from_secs(5)).await;
                }
            } else {
                sleep(Duration::from_secs(1)).await;
            }

            sleep(Duration::from_secs(1)).await;
        }
    }

    async fn check_wallets_availability(&self) -> bool {
        let wallets = self.free_wallets.lock().await;
        *wallets > 0
    }

    async fn process_message(
        &self,
        message: Result<Message, tokio_tungstenite::tungstenite::Error>,
        write: Arc<Mutex<futures_util::stream::SplitSink<
            WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
            Message,
        >>>,
        order_processor: OrderProcessor,
        free_wallets: Arc<Mutex<u32>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match message {
            Ok(Message::Text(text)) => {
                match serde_json::from_str::<MatcherResponse>(&text) {
                    Ok(MatcherResponse::Batch(orders)) if orders.is_empty() => {
                        info!("No orders available for matching. Waiting 10 seconds.");
                        sleep(Duration::from_secs(10)).await;
                    }
                    Ok(MatcherResponse::Batch(orders)) => {
                        self.process_orders(orders, write, order_processor, free_wallets).await;
                    }
                    Ok(MatcherResponse::NoOrders) => {
                        info!("Received NoOrders response. Waiting 10 seconds before retrying.");
                        sleep(Duration::from_secs(10)).await;
                    }
                    Ok(_) => {
                        info!("Received unknown response. Waiting 10 seconds before retrying.");
                        sleep(Duration::from_secs(10)).await;
                    }
                    Err(_) => {
                        error!("Failed to parse MatcherResponse: {}", text);
                    }
                }
            }
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
        write: Arc<Mutex<futures_util::stream::SplitSink<
            WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
            Message,
        >>>,
        order_processor: OrderProcessor,
        free_wallets: Arc<Mutex<u32>>,
    ) {
        let updates = match order_processor.process_orders(orders, free_wallets.clone()).await {
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
        write: Arc<Mutex<futures_util::stream::SplitSink<
            WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
            Message,
        >>>
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
