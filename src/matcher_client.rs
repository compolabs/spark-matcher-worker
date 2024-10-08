use crate::config::Settings;
use crate::error::Error;
use crate::order_processor::OrderProcessor;
use crate::types::{
    MatcherBatchRequest, MatcherConnectRequest, MatcherRequest, MatcherResponse, SpotOrder,
};
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use std::sync::Arc;
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
    free_wallets: Arc<Mutex<u32>>, // Number of free wallets
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
            free_wallets: Arc::new(Mutex::new(10)), // Initially, all 10 wallets are free
        }
    }

    pub async fn connect(&self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            match self.connect_to_ws().await {
                Ok(_) => info!("Matcher {} connected successfully!", self.uuid),
                Err(e) => error!("Failed to connect Matcher {}: {:?}", self.uuid, e),
            }
            sleep(std::time::Duration::from_secs(20)).await;
        }
    }

    async fn connect_to_ws(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Establish WebSocket connection
        let (ws_stream, _) = connect_async(&self.ws_url).await?;
        let (write, mut read) = ws_stream.split();
        let write = Arc::new(Mutex::new(write));

        // Send identification message
        let identify_message = MatcherConnectRequest {
            uuid: self.uuid.to_string(),
        };
        let msg = serde_json::to_string(&identify_message)?;
        info!("Sending identification message: {}", msg);
        {
            let mut write_lock = write.lock().await;
            write_lock.send(Message::Text(msg)).await?;
        }
        info!("Sent identification message for matcher: {}", self.uuid);

        // Main loop: request batches and handle responses
        let free_wallets = self.free_wallets.clone();
        let order_processor = self.order_processor.clone();

        loop {
            // Check for available wallets
            let wallets_available = {
                let wallets = free_wallets.lock().await;
                *wallets > 0
            };

            if wallets_available {
                // Request a batch
                self.request_batch(write.clone()).await?;

                // Wait for a response with a batch
                if let Some(message) = read.next().await {
                    match message {
                        Ok(Message::Text(text)) => {
                            if let Ok(MatcherResponse::Batch(orders)) =
                                serde_json::from_str::<MatcherResponse>(&text)
                            {
                                // Process the received batch of orders
                                let free_wallets_clone = self.free_wallets.clone();
                                let order_processor_clone = order_processor.clone();
                                let write_clone = write.clone();

                                tokio::spawn(async move {
                                    let updates = match order_processor_clone
                                        .process_orders(orders, free_wallets_clone.clone())
                                        .await
                                    {
                                        Ok(updates) => updates,
                                        Err(e) => {
                                            error!("Error processing orders: {:?}", e);
                                            Vec::new()
                                        }
                                    };

                                    // Send processing results back to the server
                                    let response = MatcherRequest::OrderUpdates(updates);
                                    let response_msg = serde_json::to_string(&response).unwrap();
                                    let mut write_lock = write_clone.lock().await;
                                    if let Err(e) = write_lock.send(Message::Text(response_msg)).await {
                                        error!("Failed to send updates to server: {:?}", e);
                                    }
                                });
                            } else {
                                error!("Failed to parse MatcherResponse");
                            }
                        }
                        Ok(Message::Close(_)) => {
                            info!("Connection closed by server. Reconnecting...");
                            break Ok(());
                        }
                        Err(e) => {
                            error!("WebSocket error: {:?}", e);
                            break Ok(());
                        }
                        _ => {
                            error!("Unexpected message format");
                        }
                    }
                } else {
                    error!("No response from server");
                    sleep(std::time::Duration::from_secs(5)).await;
                }
            } else {
                // No free wallets, wait before checking again
                sleep(std::time::Duration::from_secs(1)).await;
            }

            // Additional logic to control request frequency
            sleep(std::time::Duration::from_secs(1)).await;
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
