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

#[cfg(test)]
mod tests {
    use super::AuctionData;

    #[test]
    fn test_scale_auction() {
        let key_1 = "asset_1".to_string();
        let key_2 = "asset_2".to_string();
        let key_3 = "asset_3".to_string();

        let mut bid = std::collections::HashMap::new();
        bid.insert(key_1.clone(), 100_0000000);
        bid.insert(key_2.clone(), 200_0000001);
        let mut lot = std::collections::HashMap::new();
        lot.insert(key_2.clone(), 1_0000000);
        lot.insert(key_3.clone(), 5_0000001);
        let auction_data = AuctionData {
            bid,
            lot,
            block: 100,
        };

        // 0 blocks -> 100 percent
        let scaled_auction = auction_data.scale_auction(100, 100);
        assert_eq!(scaled_auction.block, 100);
        assert_eq!(scaled_auction.bid.len(), 2);
        assert_eq!(scaled_auction.bid.get(&key_1).unwrap().clone(), 100_0000000);
        assert_eq!(scaled_auction.bid.get(&key_2).unwrap().clone(), 200_0000001);
        assert_eq!(scaled_auction.lot.len(), 0);

        // 100 blocks -> 100 percent, validate lot is rounded down
        let scaled_auction = auction_data.scale_auction(200, 100);
        assert_eq!(scaled_auction.block, 100);
        assert_eq!(scaled_auction.bid.len(), 2);
        assert_eq!(scaled_auction.bid.get(&key_1).unwrap().clone(), 100_0000000);
        assert_eq!(scaled_auction.bid.get(&key_2).unwrap().clone(), 200_0000001);
        assert_eq!(scaled_auction.lot.len(), 2);
        assert_eq!(scaled_auction.lot.get(&key_2).unwrap().clone(), 0_5000000);
        assert_eq!(scaled_auction.lot.get(&key_3).unwrap().clone(), 2_5000000);

        // 100 blocks -> 50 percent, validate bid is rounded up
        let scaled_auction = auction_data.scale_auction(200, 50);
        assert_eq!(scaled_auction.block, 100);
        assert_eq!(scaled_auction.bid.len(), 2);
        assert_eq!(scaled_auction.bid.get(&key_1).unwrap().clone(), 50_0000000);
        assert_eq!(scaled_auction.bid.get(&key_2).unwrap().clone(), 100_0000001);
        assert_eq!(scaled_auction.lot.len(), 2);
        assert_eq!(scaled_auction.lot.get(&key_2).unwrap().clone(), 0_2500000);
        assert_eq!(scaled_auction.lot.get(&key_3).unwrap().clone(), 1_2500000);

        // 200 blocks -> 100 percent (is same)
        let scaled_auction = auction_data.scale_auction(300, 100);
        assert_eq!(scaled_auction.block, 100);
        assert_eq!(scaled_auction.bid.len(), 2);
        assert_eq!(scaled_auction.bid.get(&key_1).unwrap().clone(), 100_0000000);
        assert_eq!(scaled_auction.bid.get(&key_2).unwrap().clone(), 200_0000001);
        assert_eq!(scaled_auction.lot.len(), 2);
        assert_eq!(scaled_auction.lot.get(&key_2).unwrap().clone(), 1_0000000);
        assert_eq!(scaled_auction.lot.get(&key_3).unwrap().clone(), 5_0000001);

        // 200 blocks -> 75 percent, validate bid is rounded up and lot is rounded down
        let scaled_auction = auction_data.scale_auction(300, 75);
        assert_eq!(scaled_auction.block, 100);
        assert_eq!(scaled_auction.bid.len(), 2);
        assert_eq!(scaled_auction.bid.get(&key_1).unwrap().clone(), 75_0000000);
        assert_eq!(scaled_auction.bid.get(&key_2).unwrap().clone(), 150_0000001);
        assert_eq!(scaled_auction.lot.len(), 2);
        assert_eq!(scaled_auction.lot.get(&key_2).unwrap().clone(), 0_7500000);
        assert_eq!(scaled_auction.lot.get(&key_3).unwrap().clone(), 3_7500000);

        // 300 blocks -> 100 percent
        let scaled_auction = auction_data.scale_auction(400, 100);
        assert_eq!(scaled_auction.block, 100);
        assert_eq!(scaled_auction.bid.len(), 2);
        assert_eq!(scaled_auction.bid.get(&key_1).unwrap().clone(), 50_0000000);
        assert_eq!(scaled_auction.bid.get(&key_2).unwrap().clone(), 100_0000001);
        assert_eq!(scaled_auction.lot.len(), 2);
        assert_eq!(scaled_auction.lot.get(&key_2).unwrap().clone(), 1_0000000);
        assert_eq!(scaled_auction.lot.get(&key_3).unwrap().clone(), 5_0000001);

        // 400 blocks -> 100 percent
        let scaled_auction = auction_data.scale_auction(500, 100);
        assert_eq!(scaled_auction.block, 100);
        assert_eq!(scaled_auction.bid.len(), 0);
        assert_eq!(scaled_auction.lot.len(), 2);
        assert_eq!(scaled_auction.lot.get(&key_2).unwrap().clone(), 1_0000000);
        assert_eq!(scaled_auction.lot.get(&key_3).unwrap().clone(), 5_0000001);

        // 500 blocks -> 100 percent (unchanged)
        let scaled_auction = auction_data.scale_auction(600, 100);
        assert_eq!(scaled_auction.block, 100);
        assert_eq!(scaled_auction.bid.len(), 0);
        assert_eq!(scaled_auction.lot.len(), 2);
        assert_eq!(scaled_auction.lot.get(&key_2).unwrap().clone(), 1_0000000);
        assert_eq!(scaled_auction.lot.get(&key_3).unwrap().clone(), 5_0000001);
    }

    #[test]
    fn test_scale_auction_1_stroop_rounding() {
        let key_1 = "asset_1".to_string();
        let key_2 = "asset_2".to_string();

        let mut bid = std::collections::HashMap::new();
        bid.insert(key_1.clone(), 1);
        let mut lot = std::collections::HashMap::new();
        lot.insert(key_2.clone(), 1);
        let auction_data = AuctionData {
            bid,
            lot,
            block: 100,
        };

        // 1 blocks -> 10 percent
        let scaled_auction = auction_data.scale_auction(101, 10);
        assert_eq!(scaled_auction.block, 100);
        assert_eq!(scaled_auction.bid.len(), 1);
        assert_eq!(scaled_auction.bid.get(&key_1).unwrap().clone(), 1);
        assert_eq!(scaled_auction.lot.len(), 0);

        // 399 blocks -> 10 percent
        let scaled_auction = auction_data.scale_auction(499, 10);
        assert_eq!(scaled_auction.block, 100);
        assert_eq!(scaled_auction.bid.len(), 1);
        assert_eq!(scaled_auction.bid.get(&key_1).unwrap().clone(), 1);
        assert_eq!(scaled_auction.lot.len(), 0);

        // 399 blocks -> 100 percent
        let scaled_auction = auction_data.scale_auction(499, 100);
        assert_eq!(scaled_auction.block, 100);
        assert_eq!(scaled_auction.bid.len(), 1);
        assert_eq!(scaled_auction.bid.get(&key_1).unwrap().clone(), 1);
        assert_eq!(scaled_auction.lot.len(), 1);
        assert_eq!(scaled_auction.lot.get(&key_2).unwrap().clone(), 1);
    }
}
