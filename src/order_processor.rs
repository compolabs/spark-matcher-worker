use crate::config::Settings;
use crate::error::Error;
use crate::types::{MatcherOrderUpdate, OrderStatus, OrderType, SpotOrder};
use fuels::accounts::{provider::Provider, wallet::WalletUnlocked};
use fuels::types::ContractId;
use log::{error, info};
use spark_market_sdk::SparkMarketContract;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct OrderProcessor {
    settings: Arc<Settings>,
    hd_wallet_number: Arc<Mutex<u32>>,
}

impl OrderProcessor {
    pub fn new(settings: Arc<Settings>) -> Self {
        Self {
            settings,
            hd_wallet_number: Arc::new(Mutex::new(0)),
        }
    }

    async fn get_hd_wallet_number(&self) -> u32 {
        let mut wallet_number = self.hd_wallet_number.lock().await;
        let current_number = *wallet_number;
        *wallet_number = (*wallet_number + 1) % 10;
        current_number
    }

    pub async fn process_orders(
        &self,
        orders: Vec<SpotOrder>,
    ) -> Result<Vec<MatcherOrderUpdate>, Box<dyn std::error::Error>> {
        info!("Processing batch of {} orders", orders.len());

        let (max_buy, min_sell) = self.calculate_spread(&orders);
        info!(
            "Batch spread - Max Buy: {:?}, Min Sell: {:?}",
            max_buy, min_sell
        );

        if let (Some(max_buy), Some(min_sell)) = (max_buy, min_sell) {
            if max_buy < min_sell {
                error!(
                    "Invalid batch: Max Buy ({}) < Min Sell ({}), batch rejected",
                    max_buy, min_sell
                );

                return Err(Box::new(Error::InvalidBatch));
            }
        }

        info!("Orders:");
        for o in &orders {
            info!(
                "id: {:?} | status {:?}, | type {:?} | am {:?}",
                o.id, o.status, o.order_type, o.amount
            );
        }

        let result = self.match_orders(orders.clone()).await;

        let updates = match result {
            Ok(updates) => {
                info!("Orders processed successfully");
                updates
            }
            Err(err) => {
                error!("Error while processing orders: {:?}", err);
                orders
                    .into_iter()
                    .map(|order| MatcherOrderUpdate {
                        order_id: order.id,
                        price: order.price,
                        timestamp: order.timestamp,
                        new_amount: order.amount,
                        status: Some(OrderStatus::Failed),
                        order_type: order.order_type,
                    })
                    .collect()
            }
        };

        Ok(updates)
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

        (max_buy, min_sell)
    }

    async fn match_orders(&self, orders: Vec<SpotOrder>) -> Result<Vec<MatcherOrderUpdate>, Error> {
        let provider = Provider::connect("mainnet.fuel.network").await?;

        let hd_wallet_number = self.get_hd_wallet_number().await;
        let path = format!("m/44'/1179993420'/{}'/0/0", hd_wallet_number);
        let wallet = WalletUnlocked::new_from_mnemonic_phrase_with_path(
            &self.settings.mnemonic,
            Some(provider.clone()),
            &path,
        )
        .expect("Failed to create wallet");

        let market =
            SparkMarketContract::new(ContractId::from_str(&self.settings.contract_id)?, wallet)
                .await;

        let unique_bits256_ids: Vec<fuels::types::Bits256> = orders
            .iter()
            .map(|order| fuels::types::Bits256::from_hex_str(&order.id).unwrap())
            .collect();

        info!("Processing orders with HD wallet {}", hd_wallet_number);

        match market.match_order_many(unique_bits256_ids.clone()).await {
            Ok(result) => {
                info!(
                    "Matched orders successfully. Tx ID: {:?}, Gas used: {:?}",
                    result.tx_id, result.gas_used
                );

                let updates: Vec<MatcherOrderUpdate> = orders
                    .into_iter()
                    .map(|order| MatcherOrderUpdate {
                        order_id: order.id,
                        price: order.price,
                        timestamp: order.timestamp,
                        new_amount: 0,
                        status: Some(OrderStatus::Matched),
                        order_type: order.order_type,
                    })
                    .collect();

                Ok(updates)
            }
            Err(e) => {
                error!("Error while matching orders: {:?}", e);

                let _failed_orders: Vec<MatcherOrderUpdate> = orders
                    .into_iter()
                    .map(|order| MatcherOrderUpdate {
                        order_id: order.id,
                        price: order.price,
                        timestamp: order.timestamp,
                        new_amount: order.amount,
                        status: Some(OrderStatus::Failed),
                        order_type: order.order_type,
                    })
                    .collect();

                Err(Error::MatchOrders(e.to_string()))
            }
        }
    }
}
