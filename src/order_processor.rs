use fuels::{accounts::{provider::Provider, wallet::WalletUnlocked}, types::ContractId};
use log::{info, error};
use spark_market_sdk::SparkMarketContract;
use tokio::sync::Mutex;
use std::{collections::HashSet, str::FromStr, sync::Arc};
use crate::{config::Settings, error::Error, matcher_client::{MatcherOrderUpdate, OrderType, SpotOrder, OrderStatus}};

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

    pub async fn match_orders(&self, orders: Vec<SpotOrder>) -> Result<Vec<MatcherOrderUpdate>, Error> {
        let provider = Provider::connect("testnet.fuel.network").await?;

        
        let hd_wallet_number = self.get_hd_wallet_number().await;
        let path = format!("m/44'/1179993420'/{}'/0/0", hd_wallet_number);
        let wallet = WalletUnlocked::new_from_mnemonic_phrase_with_path(
            &self.settings.mnemonic,
            Some(provider.clone()),
            &path,
        ).expect("Failed to create wallet");

        let market = SparkMarketContract::new(ContractId::from_str(&self.settings.contract_id)?, wallet).await;

        let unique_bits256_ids: Vec<fuels::types::Bits256> = orders
            .iter()
            .map(|order| fuels::types::Bits256::from_hex_str(&order.id).unwrap())
            .collect();

        info!("Processing orders with HD wallet {}", hd_wallet_number);
/*
        info!("=======DEBUG=======");
        for o in orders.clone(){
            info!("{:?}",o.id);
            let b = fuels::types::Bits256::from_hex_str(&o.id).unwrap();
            let a = market.order(b).await.unwrap();
            info!("{:?}",a.value);
        }
        info!("=======DEBUG=======");
*/
        match market.match_order_many(unique_bits256_ids.clone()).await {
            Ok(result) => {
                info!("Matched orders successfully. Tx ID: {:?}, Gas used: {:?}", result.tx_id, result.gas_used);
                
                
                let updates: Vec<MatcherOrderUpdate> = orders.into_iter().map(|order| MatcherOrderUpdate {
                    order_id: order.id,
                    price: order.price,
                    timestamp: order.timestamp,
                    new_amount: 0, 
                    status: Some(OrderStatus::Filled), 
                    order_type: order.order_type, 
                }).collect();
                
                Ok(updates)
            }
            Err(e) => {
                error!("Error while matching orders: {:?}", e);

                
                let updates: Vec<MatcherOrderUpdate> = orders.into_iter().map(|order| MatcherOrderUpdate {
                    order_id: order.id,
                    price: order.price,
                    timestamp: order.timestamp,
                    new_amount: order.amount, 
                    status: Some(OrderStatus::Failed), 
                    order_type: order.order_type,
                }).collect();

                Err(Error::MatchOrdersError(e.to_string()))
            }
        }
    }
}
