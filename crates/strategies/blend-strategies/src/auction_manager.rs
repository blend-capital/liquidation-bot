use std::collections::HashMap;

use crate::{
    constants::SCALAR_7,
    db_manager::DbManager,
    helper::sum_adj_asset_values,
    types::{AuctionData, UserPositions},
};
use anyhow::Result;
use soroban_fixed_point_math::FixedPoint;

#[derive(Debug, Clone)]
pub struct OngoingAuction {
    pub pool: String,
    pub user: String,
    pub auction_data: AuctionData,
    pub target_block: u32,
    pub pct_to_fill: u64,
    pub pct_filled: u64,
    pub auction_type: u32,
    pub min_profit: i128,
    pub db_manager: DbManager,
    pub block_filled: u32,
}

impl OngoingAuction {
    pub fn new(
        pool: String,
        user: String,
        auction_data: AuctionData,
        auction_type: u32, //0 for liquidation, 1 for interest, 2 for bad debt
        min_profit: i128,
        db_manager: DbManager,
    ) -> Self {
        Self {
            pool,
            user,
            auction_data,
            target_block: 0,
            pct_to_fill: 0,
            pct_filled: 0,
            auction_type,
            min_profit,
            db_manager,
            block_filled: 0,
        }
    }
    pub fn calc_liquidation_fill(
        &mut self,
        our_positions: &UserPositions,
        min_hf: i128,
    ) -> Result<i128> {
        let (collateral_value, adjusted_collateral_value) = sum_adj_asset_values(
            self.auction_data.lot.clone(),
            &self.pool,
            true,
            &self.db_manager,
        )?;
        let (liabilities_value, adjusted_liability_value) = sum_adj_asset_values(
            self.auction_data.bid.clone(),
            &self.pool,
            false,
            &self.db_manager,
        )?;

        let (_, our_collateral) = sum_adj_asset_values(
            our_positions.collateral.clone(),
            &self.pool,
            true,
            &self.db_manager,
        )?;
        let (_, our_debt) = sum_adj_asset_values(
            our_positions.liabilities.clone(),
            &self.pool,
            false,
            &self.db_manager,
        )?;

        Ok(self.set_percent_and_target(
            collateral_value,
            liabilities_value,
            adjusted_liability_value,
            adjusted_collateral_value,
            get_max_delta_hf(our_collateral, our_debt, adjusted_liability_value, min_hf),
        ))
    }
    pub fn calc_interest_fill(
        &mut self,
        our_backstop_tokens: i128,
        backstop_token: String,
        bid_value: i128,
    ) -> Result<i128> {
        let (lot_value, _) = sum_adj_asset_values(
            self.auction_data.lot.clone(),
            &self.pool,
            true,
            &self.db_manager,
        )?;
        let num_backstop_tokens = self.auction_data.bid.get(&backstop_token).unwrap();

        Ok(self.set_percent_and_target(
            lot_value,
            bid_value,
            num_backstop_tokens.clone(),
            0,
            our_backstop_tokens,
        ))
    }
    pub fn calc_bad_debt_fill(
        &mut self,
        db_manager: &DbManager,
        our_wallet: &HashMap<String, i128>,
        lot_value: i128,
    ) -> Result<i128> {
        let (liabilities_value, _) = sum_adj_asset_values(
            self.auction_data.bid.clone(),
            &self.pool,
            true,
            &self.db_manager,
        )?;

        let mut worst_ratio = 0;
        let mut worst_bid_value = 0;
        let mut worst_wallet_balance = 0;
        self.auction_data.bid.iter().for_each(|(asset, bid_value)| {
            let wallet_balance = our_wallet.get(asset).unwrap_or(&0);
            let bid_val_in_raw = bid_value
                .fixed_mul_floor(
                    db_manager
                        .get_reserve_config_from_asset(&self.pool, asset)
                        .unwrap()
                        .est_d_rate,
                    SCALAR_7,
                )
                .unwrap();
            let ratio = bid_val_in_raw
                .fixed_div_floor(wallet_balance.clone(), SCALAR_7)
                .unwrap_or(0);
            if ratio > worst_ratio {
                worst_ratio = ratio;
                worst_bid_value = bid_value.clone() + 10;
                worst_wallet_balance = wallet_balance.clone();
            }
        });

        Ok(self.set_percent_and_target(
            lot_value,
            liabilities_value,
            worst_bid_value,
            0,
            worst_wallet_balance,
        ))
    }
    pub fn partial_fill_update(&mut self, fill_percentage: u64) {
        //Update pct_filled for pending fill
        let old_pct_filled = self.pct_filled.clone();
        self.pct_filled = old_pct_filled
            + (100 - old_pct_filled)
                .fixed_mul_ceil(fill_percentage, 100)
                .unwrap()
                .clamp(0, 99);
        //Update pct_to_fill for pending fill
        let old_pct_to_fill = self.pct_to_fill.clone();
        self.pct_to_fill = old_pct_to_fill
            .fixed_div_floor(100 - fill_percentage as u64, 100)
            .unwrap()
            .clamp(0, 100);
    }
    // Sets the percent to fill and target block for the auction
    // Returns expected profit at target block
    fn set_percent_and_target(
        &mut self,
        mut lot_value: i128,
        mut bid_value: i128,
        mut raw_bid_required: i128,
        mut bid_offset: i128,
        our_max_bid: i128,
    ) -> i128 {
        // apply pct_filled
        let pct_remaining = 100 - self.pct_filled as i128;
        lot_value = lot_value.fixed_mul_floor(pct_remaining, 100).unwrap();
        bid_value = bid_value.fixed_mul_ceil(pct_remaining, 100).unwrap();
        raw_bid_required = raw_bid_required.fixed_mul_ceil(pct_remaining, 100).unwrap();
        bid_offset = bid_offset.fixed_mul_floor(pct_remaining, 100).unwrap();

        if our_max_bid == 0 {
            self.pct_to_fill = 100;
            self.target_block = self.auction_data.block + 400;
            return lot_value;
        }
        let (fill_block, mut profit) = get_fill_info(self.min_profit, lot_value, bid_value);
        let bid_required = get_bid_required(fill_block, raw_bid_required, bid_offset);
        self.target_block = fill_block as u32 + self.auction_data.block;
        self.pct_to_fill = if our_max_bid >= bid_required {
            100
        } else {
            let pct = our_max_bid.fixed_div_floor(bid_required, 100).unwrap() as i128;
            profit = profit.fixed_mul_floor(pct, 100).unwrap();
            let profit_dif = self.min_profit - profit;
            if profit_dif > 0 {
                let profit_per_block = lot_value
                    .fixed_mul_floor(pct, 100)
                    .unwrap()
                    .fixed_div_floor(200, 1)
                    .unwrap();
                let additional_blocks = profit_dif.fixed_div_ceil(profit_per_block, 1).unwrap();
                self.target_block += additional_blocks as u32;
                profit += profit_per_block
                    .fixed_mul_floor(additional_blocks, 1)
                    .unwrap();
            }
            pct as u64
        };
        profit
    }
}

// returns the block we should bid at and the expected profit at that block
fn get_fill_info(min_profit: i128, lot_value: i128, bid_value: i128) -> (i128, i128) {
    let mut mod_lot_value = 0;
    let mut mod_bid_value = bid_value.clone();
    let step_lot_value = lot_value / 200;
    let step_bid_value = bid_value / 200;
    for i in 1..400 {
        if i <= 200 {
            mod_lot_value += step_lot_value;
        } else {
            mod_bid_value -= step_bid_value;
        }
        let profit = mod_lot_value - mod_bid_value;
        if profit >= min_profit {
            return (i, profit);
        }
    }
    (400, lot_value)
}

//TODO: this should take into account crossing positions and net them if inventory management is implemented
fn get_max_delta_hf(collateral: i128, debt: i128, new_debt: i128, min_hf: i128) -> i128 {
    if debt == 0 {
        collateral
    } else {
        // collateral/min_hf - debt = how much additional debt we can take on while remaining healthy
        (collateral.fixed_div_floor(min_hf, SCALAR_7).unwrap() - debt).clamp(0, new_debt)
    }
}

fn get_bid_required(fill_block: i128, raw_bid_required: i128, bid_offset: i128) -> i128 {
    if fill_block > 200 {
        raw_bid_required
            .fixed_mul_ceil(1e7 as i128 - 0_005_0000 * (fill_block - 200), SCALAR_7)
            .unwrap()
            - bid_offset
    } else {
        raw_bid_required
            - bid_offset
                .fixed_mul_floor(0_005_0000 * fill_block, SCALAR_7)
                .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use crate::constants::SCALAR_7;
    use crate::db_manager::DbManager;
    use crate::types::AuctionData;

    #[test]
    fn test_max_delta() {
        //set up test
        let debt = 125 * SCALAR_7;
        let collateral = 240 * SCALAR_7;
        let new_debt = 100 * SCALAR_7;
        let min_hf = 1_200_0000;
        let max = super::get_max_delta_hf(collateral, debt, new_debt, min_hf);
        assert_eq!(max, 75_000_0000);
    }

    #[test]
    fn test_fill_info() {
        //set up test
        let min_profit = 10 * SCALAR_7;
        let lot_value = 200 * SCALAR_7;
        let bid_value = 100 * SCALAR_7;
        let fill_info = super::get_fill_info(min_profit, lot_value, bid_value);
        assert_eq!(fill_info, (110, 10 * SCALAR_7));
    }

    #[test]
    fn test_get_bid_req_u_200() {
        //set up test
        let fill_block = 150;
        let raw_bid_required = 200 * SCALAR_7;
        let bid_offset = 100 * SCALAR_7;
        let bid_req = super::get_bid_required(fill_block, raw_bid_required, bid_offset);
        assert_eq!(bid_req, 125 * SCALAR_7);
    }
    #[test]
    fn test_get_bid_req_o_200() {
        //set up test
        let fill_block = 225;
        let raw_bid_required = 200 * SCALAR_7;
        let bid_offset = 100 * SCALAR_7;
        let bid_req = super::get_bid_required(fill_block, raw_bid_required, bid_offset);
        assert_eq!(bid_req, 75 * SCALAR_7);
    }

    #[test]
    fn test_set_pct_target() {
        //set up test
        let mut auction = super::OngoingAuction::new(
            "CBFG6XIGMSUUEQRMBM7G4RSLPYPVIC6WYHC2XVKSNBFET4S3IBZA6TNQ".to_string(),
            "test".to_string(),
            AuctionData {
                block: 300,
                lot: Default::default(),
                bid: Default::default(),
            },
            0,
            10 * SCALAR_7,
            DbManager::new("test".to_string()),
        );
        auction.pct_filled = 50;
        let profit = auction.set_percent_and_target(
            400 * SCALAR_7,
            200 * SCALAR_7,
            200 * SCALAR_7,
            184 * SCALAR_7,
            37_050_0000,
        );
        assert_eq!(auction.target_block, 414);
        assert_eq!(auction.pct_to_fill, 75);
        assert_eq!(profit, 10_500_0000);
    }
    #[test]
    fn test_set_pct_target_100() {
        //set up test
        let mut auction = super::OngoingAuction::new(
            "CBFG6XIGMSUUEQRMBM7G4RSLPYPVIC6WYHC2XVKSNBFET4S3IBZA6TNQ".to_string(),
            "test".to_string(),
            AuctionData {
                block: 300,
                lot: Default::default(),
                bid: Default::default(),
            },
            0,
            10 * SCALAR_7,
            DbManager::new("test".to_string()),
        );
        auction.pct_filled = 0;
        let profit = auction.set_percent_and_target(
            200 * SCALAR_7,
            220 * SCALAR_7,
            200 * SCALAR_7,
            90 * SCALAR_7,
            100 * SCALAR_7,
        );
        assert_eq!(auction.target_block, 528);
        assert_eq!(auction.pct_to_fill, 100);
        assert_eq!(profit, 10_800_0000);
    }
}
