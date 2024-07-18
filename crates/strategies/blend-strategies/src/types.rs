use std::collections::HashMap;

use artemis_core::{
    collectors::block_collector::NewBlock, executors::soroban_executor::SubmitStellarTx,
};
use serde::Deserialize;
use soroban_fixed_point_math::FixedPoint;
use stellar_rpc_client::Event as SorobanEvent;

use crate::constants::SCALAR_7;
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
    pub slack_api_url_key: String,
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

impl AuctionData {
    pub fn scale_auction(&self, fill_block: u32, percent_filled: u64) -> Self {
        let mut scaled_auction = AuctionData {
            bid: HashMap::<String, i128>::new(),
            lot: HashMap::<String, i128>::new(),
            block: self.block,
        };

        // determine block based auction modifiers
        let bid_modifier: i128;
        let lot_modifier: i128;
        let per_block_scalar: i128 = 0_0050000; // modifier moves 0.5% every block
        let block_dif = (fill_block - self.block) as i128;
        if block_dif > 200 {
            // lot 100%, bid scaling down from 100% to 0%
            lot_modifier = SCALAR_7;
            if block_dif < 400 {
                bid_modifier = SCALAR_7 - (block_dif - 200) * per_block_scalar;
            } else {
                bid_modifier = 0;
            }
        } else {
            // lot scaling from 0% to 100%, bid 100%
            lot_modifier = block_dif * per_block_scalar;
            bid_modifier = SCALAR_7;
        }

        // scale the auction
        let percent_filled_i128 = (percent_filled as i128) * 1_00000; // scale to decimal form in 7 decimals from percentage
        for (asset, amount) in self.bid.iter() {
            // apply percent scalar and store remainder to base auction
            // round up to avoid rounding exploits
            let bid_base = amount
                .fixed_mul_ceil(percent_filled_i128, SCALAR_7)
                .unwrap();
            // apply block scalar to to_fill auction and don't store if 0
            let bid_scaled = bid_base.fixed_mul_ceil(bid_modifier, SCALAR_7).unwrap();
            if bid_scaled > 0 {
                scaled_auction.bid.insert(asset.clone(), bid_scaled);
            }
        }
        for (asset, amount) in self.lot.iter() {
            // apply percent scalar and store remainder to base auction
            // round down to avoid rounding exploits
            let lot_base = amount
                .fixed_mul_floor(percent_filled_i128, SCALAR_7)
                .unwrap();
            // apply block scalar to to_fill auction and don't store if 0
            let lot_scaled = lot_base.fixed_mul_floor(lot_modifier, SCALAR_7).unwrap();
            if lot_scaled > 0 {
                scaled_auction.lot.insert(asset.clone(), lot_scaled);
            }
        }

        return scaled_auction;
    }
}
