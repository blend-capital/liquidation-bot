use std::collections::HashMap;

use stellar_xdr::curr::Hash;

use crate::{
    helper::sum_adj_asset_values,
    types::{AuctionData, ReserveConfig, UserPositions},
};

#[derive(Debug, Clone)]
pub struct OngoingAuction {
    pub pool: Hash,
    pub user: Hash,
    pub auction_data: AuctionData,
    pub target_block: u32,
    pub pct_to_fill: u64,
    pub pct_filled: u64,
    pub auction_type: u32,
    pub target_profit_pct: i128,
}

impl OngoingAuction {
    pub fn new(
        pool: Hash,
        user: Hash,
        auction_data: AuctionData,
        auction_type: u32, //0 for liquidation, 1 for interest, 2 for bad debt
    ) -> Self {
        Self {
            pool,
            user,
            auction_data,
            target_block: 0,
            pct_to_fill: 0,
            pct_filled: 0,
            auction_type,
            target_profit_pct: 0,
        }
    }
    pub fn calc_liquidation_fill(
        &mut self,
        prices: &HashMap<Hash, i128>,
        reserve_configs: &HashMap<Hash, ReserveConfig>,
        our_positions: &UserPositions,
        min_hf: i128,
        min_profit: i128,
    ) {
        let (collateral_value, adjusted_collateral_value) =
            sum_adj_asset_values(self.auction_data.lot.clone(), reserve_configs, prices, true);
        let (liabilities_value, adjusted_liability_value) =
            sum_adj_asset_values(self.auction_data.bid.clone(), reserve_configs, prices, true);

        self.target_profit_pct = min_profit / liabilities_value;

        let (_, our_collateral) = sum_adj_asset_values(
            our_positions.collateral.clone(),
            reserve_configs,
            prices,
            true,
        );
        let (_, our_debt) = sum_adj_asset_values(
            our_positions.liabilities.clone(),
            reserve_configs,
            prices,
            false,
        );

        //TODO: this should take into account crossing positions and net them when possible (ie. user deposited collateral of the same type to pay off deposited debt)
        let max_delta = if our_debt == 0 {
            our_collateral
        } else {
            (min_hf - our_collateral * 1e7 as i128 / our_debt) * our_debt
        };

        self.set_percent_and_target(
            collateral_value,
            liabilities_value,
            adjusted_liability_value,
            adjusted_collateral_value,
            max_delta,
        )
    }
    pub fn calc_interest_fill(
        &mut self,
        prices: &HashMap<Hash, i128>,
        our_backstop_tokens: i128,
        backstop_token: Hash,
        bid_value: i128,
        min_profit: i128,
    ) {
        let mut lot_value: i128 = 0;
        for (asset, amount) in self.auction_data.lot.iter() {
            lot_value += amount * prices.get(asset).unwrap() / 1e7 as i128;
        }
        let num_backstop_tokens = self.auction_data.bid.get(&backstop_token).unwrap();

        self.target_profit_pct = min_profit / bid_value;
        self.set_percent_and_target(
            lot_value,
            bid_value,
            num_backstop_tokens.clone(),
            0,
            our_backstop_tokens,
        )
    }
    pub fn calc_bad_debt_fill(
        &mut self,
        prices: &HashMap<Hash, i128>,
        reserve_configs: &HashMap<Hash, ReserveConfig>,
        our_positions: &UserPositions,
        min_hf: i128,
        min_profit: i128,
        lot_value: i128,
    ) {
        let (liabilities_value, adjusted_liability_value) =
            sum_adj_asset_values(self.auction_data.bid.clone(), reserve_configs, prices, true);

        self.target_profit_pct = min_profit / liabilities_value;

        let (_, our_collateral) = sum_adj_asset_values(
            our_positions.collateral.clone(),
            reserve_configs,
            prices,
            true,
        );
        let (_, our_debt) = sum_adj_asset_values(
            our_positions.liabilities.clone(),
            reserve_configs,
            prices,
            false,
        );
        //TODO: this should take into account crossing positions and net them when possible (ie. user deposited collateral of the same type to pay off deposited debt)
        let max_delta = (min_hf - our_collateral * 1e7 as i128 / our_debt) * our_debt;

        self.set_percent_and_target(
            lot_value,
            liabilities_value,
            adjusted_liability_value,
            0,
            max_delta,
        )
    }
    pub fn partial_fill_update(&mut self, fill_percentage: u64) {
        //Update pct_filled for pending fill
        let old_pct_filled = self.pct_filled.clone();
        self.pct_filled = old_pct_filled + (100 - old_pct_filled) * (fill_percentage as u64) / 100;

        //Update pct_to_fill for pending fill
        let old_pct_to_fill = self.pct_to_fill.clone();
        self.pct_to_fill = old_pct_to_fill * 100 / (100 - fill_percentage as u64);
    }
    // returns (target block,percent to fill)
    //TODO: once price quoter supports liquidity considerations this the percent_fill should influence that bid_block since a smaller fill lowers liquidity requirements
    fn set_percent_and_target(
        &mut self,
        mut lot_value: i128,
        mut bid_value: i128,
        mut raw_bid_required: i128,
        mut bid_offset: i128,
        our_max_bid: i128,
    ) {
        // apply pct_filled
        lot_value = lot_value * (100 - self.pct_filled as i128) / 100;
        bid_value = bid_value * (100 - self.pct_filled as i128) / 100;
        raw_bid_required = raw_bid_required * (100 - self.pct_filled as i128) / 100;
        bid_offset = bid_offset * (100 - self.pct_filled as i128) / 100;

        let profit_dif: i128 =
            self.target_profit_pct - (lot_value - bid_value) * 1e7 as i128 / lot_value;
        let target_block_dif = profit_dif as f64 / 0_005_0000 as f64; // profit increases .05% per block
        if target_block_dif > 0.0 {
            target_block_dif.ceil()
        } else {
            target_block_dif.floor()
        };
        println!("target_block_dif: {}", target_block_dif);
        let target_block_dif = if target_block_dif < 200.0 {
            target_block_dif as i128
        } else {
            200
        };
        let bid_required = if target_block_dif > 0 {
            raw_bid_required * (1e7 as i128 - 0_005_0000 * target_block_dif) - bid_offset
        } else {
            raw_bid_required - bid_offset * (1e7 as i128 + 0_005_0000 * target_block_dif)
        };
        self.pct_to_fill = if our_max_bid > bid_required {
            100
        } else {
            (our_max_bid * 1e7 as i128 / bid_required / 1e5 as i128) as u64
        };
        self.target_block = (target_block_dif + 200) as u32 + self.auction_data.block;
    }
}
