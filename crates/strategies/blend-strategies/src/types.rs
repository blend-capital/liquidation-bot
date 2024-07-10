use std::collections::HashMap;

use artemis_core::{
    collectors::block_collector::NewBlock, executors::soroban_executor::SubmitStellarTx,
};
use serde::Deserialize;
use stellar_rpc_client::Event as SorobanEvent;
/// Core Event enum for the current strategy.
#[derive(Debug, Clone)]
pub enum Event {
    SorobanEvents(Box<SorobanEvent>),
    NewBlock(Box<NewBlock>),
}

/// Core Action enum for the current strategy.
#[derive(Debug, Clone)]
pub enum Action {
    SubmitTx(SubmitStellarTx),
}

/// Configuration for variables we need to pass to the strategy.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub rpc_url: String,
    pub network_passphrase: String,
    pub db_path: String,
    pub slack_api_key: String,
    pub pools: Vec<String>,
    pub supported_collateral: Vec<String>,
    pub supported_liabilities: Vec<String>,
    pub backstop: String,
    pub backstop_token_address: String,
    pub usdc_token_address: String,
    pub xlm_address: String,
    pub bid_percentage: u64,
    pub oracle_id: String,
    pub min_hf: i128,
    pub required_profit: i128,
    pub oracle_decimals: u32,
}
#[derive(Debug, Clone, Deserialize)]
pub struct PendingFill {
    pub pool: String,
    pub user: String,
    pub collateral: HashMap<String, i128>,
    pub liabilities: HashMap<String, i128>,
    pub pct_filled: u64,
    pub target_block: u32,
    pub auction_type: u8,
}
#[derive(Debug, Clone)]
pub struct UserPositions {
    pub collateral: HashMap<String, i128>,
    pub liabilities: HashMap<String, i128>,
}

#[derive(Debug, Clone)]
pub struct ReserveConfig {
    pub asset: String,
    pub index: u32,
    pub liability_factor: u32,
    pub collateral_factor: u32,
    pub est_b_rate: i128,
    pub est_d_rate: i128,
    pub scalar: i128,
}
impl ReserveConfig {
    pub fn default(asset: String) -> Self {
        ReserveConfig {
            asset,
            index: Default::default(),
            liability_factor: Default::default(),
            collateral_factor: Default::default(),
            est_b_rate: Default::default(),
            est_d_rate: Default::default(),
            scalar: Default::default(),
        }
    }

    pub fn new(
        asset: String,
        index: u32,
        liability_factor: u32,
        collateral_factor: u32,
        est_b_rate: i128,
        est_d_rate: i128,
        scalar: i128,
    ) -> Self {
        ReserveConfig {
            asset,
            index,
            liability_factor,
            collateral_factor,
            est_b_rate,
            est_d_rate,
            scalar,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuctionData {
    pub bid: HashMap<String, i128>, //liabilities || backstop_token || bad_debt
    pub lot: HashMap<String, i128>, //collateral || interest || bad_debt
    pub block: u32,
}
