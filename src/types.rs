use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

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

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct MatcherOrderUpdate {
    pub order_id: String,
    pub price: u128,
    pub timestamp: u64,
    pub new_amount: u128,
    pub status: Option<OrderStatus>,
    pub order_type: OrderType,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MatcherConnectRequest {
    pub uuid: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum MatcherResponse {
    Batch(Vec<SpotOrder>),
    Ack, 
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MatcherBatchRequest {
    pub uuid: String,
}


#[derive(Debug, Serialize, Deserialize)]
pub enum MatcherRequest {
    BatchRequest(MatcherBatchRequest),
    OrderUpdates(Vec<MatcherOrderUpdate>), // Для отправки результатов обработки серверу
}
