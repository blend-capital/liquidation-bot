use artemis_core::collectors::block_collector::NewBlock;
use artemis_core::executors::soroban_executor::SubmitStellarTx;
use async_trait::async_trait;
use std::collections::HashMap;
use std::str::FromStr;
use std::vec;
use stellar_strkey::{ed25519::PublicKey as Ed25519PublicKey, Strkey};

use crate::auction_manager::OngoingAuction;
use crate::helper::{
    decode_auction_data, decode_scaddress_to_hash, evaluate_user, reserve_config_from_ledger_entry,
    reserve_data_from_ledger_entry, user_positions_from_ledger_entry,
};
use crate::transaction_builder::{BlendTxBuilder, Request};
use ed25519_dalek::SigningKey;
use soroban_cli::utils::contract_id_from_str;
use stellar_strkey::ed25519::PrivateKey;
use stellar_xdr::curr::{
    AccountId, Hash, InvokeContractArgs, InvokeHostFunctionOp, LedgerEntryData, LedgerKey,
    LedgerKeyContractData, Limits, Memo, MuxedAccount, Operation, Preconditions, PublicKey,
    ReadXdr, ScAddress, ScSymbol, ScVal, ScVec, StringM, Transaction, TransactionEnvelope,
    TransactionV1Envelope, Uint256, VecM,
};
use tracing::info;

use super::helper::decode_entry_key;
use super::types::{Action, Event};
use crate::types::{Config, ReserveConfig, UserPositions};
use anyhow::Result;
use artemis_core::types::Strategy;
use soroban_cli::rpc::{Client, Event as SorobanEvent};
pub struct BlendLiquidator {
    /// Soroban RPC client for interacting with chain
    rpc: Client,
    /// Assets we're interested in
    assets: Vec<Hash>,
    /// Map Assets to bid on and their prices - TODO: update this to take into account slippage
    asset_prices: HashMap<Hash, i128>,
    /// Vec of Blend pool addresses to bid on auctions in
    pools: Vec<Hash>,
    /// Backstop ID
    backstop_id: Hash,
    /// Oracle ID for getting asset prices
    oracle_id: Hash,
    /// Amount of profits to bid in gas
    bid_percentage: u64,
    /// Required profitability for auctions
    required_profit: i128,
    /// Pending auction fills
    pending_fill: Vec<OngoingAuction>,
    /// All protocol users
    all_user: Vec<Hash>,
    /// Map pool users and their positions
    /// - only stores users with health factor < 5
    /// - only stores users with relevant assets
    /// HashMap<PoolId, HashMap<UserId, UserPositions>>
    users: Box<HashMap<Hash, HashMap<Hash, UserPositions>>>,
    /// Map of pools and their reserve configurations
    /// HashMap<PoolId,HasMap<AssetId, ReserveConfig>>
    reserve_configs: Box<HashMap<Hash, HashMap<Hash, ReserveConfig>>>,
    /// Our positions
    bankroll: HashMap<Hash, UserPositions>,
    /// Our wallet
    wallet: HashMap<Hash, i128>,
    /// Our signing address
    us: SigningKey,
    /// Our public key
    us_public: Hash,
    /// Our sequence number
    sequence_num: i64,
    // Our minimum health factor
    min_hf: i128,
    // Backstop token address
    backstop_token_address: Hash,
}

impl BlendLiquidator {
    pub async fn new(config: &Config) -> Self {
        let us = SigningKey::from_bytes(&PrivateKey::from_string(&config.us).unwrap().0);
        let first_user = SigningKey::from_bytes(
            &PrivateKey::from_string(&"SAHOFD3SEI4NWS2OXTXDMWJHYJ3C4V4PXM3JFY5MPOMHVCWTVO7ZVU6I")
                .unwrap()
                .0,
        );
        Self {
            rpc: Client::new(config.rpc_url.as_str()).unwrap(),
            assets: config.assets.clone(),
            asset_prices: HashMap::new(),
            pools: config.pools.clone(),
            backstop_id: config.backstop.clone(),
            oracle_id: config.oracle_id.clone(),
            bid_percentage: config.bid_percentage,
            required_profit: config.required_profit,
            pending_fill: vec![],
            all_user: vec![Hash(first_user.verifying_key().as_bytes().clone())], //TODO decide where we're getting this list from
            users: Box::new(HashMap::new()),
            reserve_configs: Box::new(HashMap::new()),
            bankroll: HashMap::new(),
            wallet: HashMap::new(), //TODO: need to pull this
            us: us.clone(),
            us_public: Hash(us.verifying_key().as_bytes().clone()),
            sequence_num: 0,
            min_hf: config.min_hf,
            backstop_token_address: config.backstop_token_address.clone(),
        }
    }
}

#[async_trait]
impl Strategy<Event, Action> for BlendLiquidator {
    async fn sync_state(&mut self) -> Result<()> {
        // TODO: maybe updated missed users since last block this was run on
        // TODO: should pull in current auctions if possible

        //get sequence num
        self.sequence_num = self
            .rpc
            .get_account(
                &Strkey::PublicKeyEd25519(Ed25519PublicKey(self.us.verifying_key().to_bytes()))
                    .to_string(),
            )
            .await
            .unwrap()
            .seq_num
            .into();

        // Get all asset prices
        self.get_asset_prices(self.assets.clone()).await?;

        // Get reserve configs for given pools
        self.get_reserve_config(self.assets.clone()).await;

        for pool in self.pools.clone() {
            // Get our positions
            self.get_user_position(pool.clone(), self.us_public.clone())
                .await?;
            // Get all users
            for user in self.all_user.clone() {
                self.get_user_position(pool.clone(), user.clone()).await?;
            }
        }

        info!("done syncing state");
        println!("pool count, {}", self.users.len());
        for pool in self.pools.iter() {
            let user_count = self.users.get(pool).unwrap().len();
            let pool_str = pool.to_string();
            info!(
                "found {:?} relevant users for pool {:?}",
                user_count, pool_str
            );
        }

        Ok(())
    }

    // Process incoming events, filter non-auction events, decide if we care about auctions
    async fn process_event(&mut self, event: Event) -> Vec<Action> {
        //
        let mut actions: Vec<Action> = [].to_vec();
        match event {
            Event::SorobanEvents(events) => {
                info!("new soroban event");
                let events = *events;
                self.process_soroban_events(events, &mut actions).await;
            }
            Event::NewBlock(block) => {
                info!("new block event");
                self.process_new_block_event(*block, &mut actions).await;
            }
        }
        actions
    }
}

impl BlendLiquidator {
    // Process new orders as they come in.
    async fn process_soroban_events(
        &mut self,
        event: SorobanEvent,
        actions: &mut Vec<Action>,
    ) -> Option<Vec<Action>> {
        println!("new soroban event");
        //should build pending auctions and remove or modify pending auctions that are filled or partially filled by someone else
        let pool_id = Hash(contract_id_from_str(&event.contract_id).unwrap());
        let mut name: String = Default::default();
        //Get contract function name from topics
        let topic = ScVal::from_xdr_base64(event.topic[0].as_bytes(), Limits::none()).unwrap();
        match topic {
            ScVal::Symbol(function_name) => {
                name = function_name.0.to_string();
            }
            _ => (),
        }
        let data = ScVal::from_xdr_base64(event.value.as_bytes(), Limits::none()).unwrap();
        println!("name {}", name.as_str());
        //Deserialize event body cases
        match name.as_str() {
            "new_liquidation_auction" => {
                let user = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap(),
                );

                let auction_data = decode_auction_data(data);

                if self.validate_assets(auction_data.lot.clone(), auction_data.bid.clone()) {
                    let mut pending_fill = OngoingAuction::new(
                        pool_id.clone(),
                        user.clone(),
                        auction_data.clone(),
                        0,
                        self.min_hf,
                    );
                    pending_fill.calc_liquidation_fill(
                        &self.asset_prices,
                        self.reserve_configs.get(&pool_id).unwrap(),
                        self.bankroll.get(&pool_id).unwrap(),
                        self.min_hf,
                    );
                    self.pending_fill.push(pending_fill);
                    // remove user from users list since they are being liquidated
                    self.users.entry(pool_id.clone()).or_default().remove(&user);
                }
            }
            "delete_liquidation_auction" => {
                // If this was an auction we were planning on filling, remove it from the pending list
                let user = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap(),
                );
                for (index, pending_fill) in self.pending_fill.clone().iter().enumerate() {
                    if pending_fill.user.0 == user.0 {
                        self.pending_fill.remove(index);
                        // add user back to users
                        self.get_user_position(pool_id.clone(), user.clone())
                            .await
                            .unwrap();
                    }
                }
            }
            "new_auction" => {
                let mut auction_type = 0;
                match ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap() {
                    ScVal::U32(num) => {
                        auction_type = num;
                    }
                    _ => (),
                }
                let auction_data = decode_auction_data(data);

                if self.validate_assets(auction_data.lot.clone(), auction_data.bid.clone()) {
                    let mut pending_fill = OngoingAuction::new(
                        pool_id.clone(),
                        self.backstop_id.clone(),
                        auction_data.clone(),
                        auction_type,
                        self.min_hf,
                    );
                    //Bad debt auction
                    if auction_type == 1 {
                        pending_fill.calc_bad_debt_fill(
                            &self.asset_prices,
                            self.reserve_configs.get(&pool_id).unwrap(),
                            self.bankroll.get(&self.us_public).unwrap(),
                            self.min_hf,
                            self.backstop_token_address.clone(),
                        )
                    } else {
                        //Interest auction
                        pending_fill.calc_interest_fill(
                            &self.asset_prices,
                            self.backstop_token_address.clone(),
                            self.wallet
                                .get(&self.backstop_token_address)
                                .unwrap()
                                .clone(),
                        )
                    }
                    self.pending_fill.push(pending_fill);
                }
            }
            "fill_auction" => {
                let liquidated_id = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap(),
                );
                let mut auction_type = 0;
                match ScVal::from_xdr_base64(event.topic[2].as_bytes(), Limits::none()).unwrap() {
                    ScVal::U32(num) => {
                        auction_type = num;
                    }
                    _ => (),
                }
                let fill_percentage: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(1).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => (&amount).into(),
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };

                let liquidator_id: Hash = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let id = vec.clone().get(0).unwrap().to_owned();
                            decode_scaddress_to_hash(&id)
                        } else {
                            Hash([0; 32])
                        }
                    }
                    _ => Hash([0; 32]),
                };
                // if we filled update our bankroll
                if liquidator_id.0 == self.us_public.0 {
                    self.get_user_position(pool_id.clone(), self.us_public.clone())
                        .await
                        .unwrap();
                }

                for (index, pending_fill) in self.pending_fill.clone().iter_mut().enumerate() {
                    if pending_fill.user == liquidated_id
                        && pending_fill.pool == pool_id
                        && pending_fill.auction_type == auction_type
                    {
                        if fill_percentage == 100 {
                            self.pending_fill.remove(index);

                            // add user back to positions
                            self.get_user_position(pool_id.clone(), liquidated_id.clone())
                                .await
                                .unwrap();
                            //check if a bad debt call is necessary
                            let current_block =
                                self.rpc.get_latest_ledger().await.unwrap().sequence;

                            if self
                                .users
                                .get(&pool_id)
                                .unwrap()
                                .get(&liquidated_id)
                                .is_none()
                                && auction_type == 0
                                && current_block - pending_fill.auction_data.block > 200
                            {
                                // Code to execute if the value is None
                                self.sequence_num += 1;
                                let tx_builder = BlendTxBuilder {
                                    contract_id: pool_id.clone(),
                                    signing_key: self.us.clone(),
                                };
                                actions.push(Action::SubmitTx(SubmitStellarTx {
                                    tx: tx_builder
                                        .bad_debt(self.sequence_num, liquidated_id.clone())
                                        .unwrap(),
                                    gas_bid_info: None,
                                    signing_key: self.us.clone(),
                                }));
                                self.sequence_num += 1;
                                actions.push(Action::SubmitTx(SubmitStellarTx {
                                    tx: tx_builder
                                        .new_auction(self.sequence_num, self.backstop_id.clone(), 1)
                                        .unwrap(),
                                    gas_bid_info: None,
                                    signing_key: self.us.clone(),
                                }));
                            }
                        } else {
                            pending_fill.partial_fill_update(fill_percentage as u64);
                        }
                        break;
                    }
                }
            }
            "set_reserve" => {
                let mut asset_id: Hash = Hash([0; 32]);
                match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let address = vec.clone().get(0).unwrap().to_owned();
                            match address {
                                ScVal::Address(_) => {
                                    asset_id = decode_scaddress_to_hash(&address);
                                }
                                _ => (),
                            }
                        } else {
                            ();
                        }
                    }
                    _ => (),
                }
                // Update the reserve config for the pool
                self.get_reserve_config(vec![asset_id]).await; //TODO: don't think this is necessary the config should be in the event
            }
            "supply" => {
                let asset_id = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap(),
                );

                let b_tokens_minted: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(1).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => (&amount).into(),
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };
                let supply_amount: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(0).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => (&amount).into(),
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };
                // Update reserve estimated b rate by using request.amount/b_tokens_minted from the emitted event
                self.update_rate(pool_id, asset_id, supply_amount, b_tokens_minted, true)
            }
            "withdraw" => {
                let asset_id = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap(),
                );
                let b_tokens_burned: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(1).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => (&amount).into(),
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };
                let withdraw_amount: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(0).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => (&amount).into(),
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };

                // Update reserve estimated b rate by using tokens out/b tokens burned from the emitted event
                self.update_rate(pool_id, asset_id, withdraw_amount, b_tokens_burned, true)
            }
            "supply_collateral" => {
                let asset_id = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap(),
                );
                let user = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[2].as_bytes(), Limits::none()).unwrap(),
                );
                let supply_amount: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(0).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => (&amount).into(),
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };
                let b_tokens_minted: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(1).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => (&amount).into(),
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };
                println!(
                    "Supply collateral!\nasset address:{:?}\nuser address: {:?}\nsupply amount: {:?}\nb tokens minted: {:?}",
                    asset_id,
                    user,
                    supply_amount,
                    b_tokens_minted
                );
                self.update_user(&pool_id, &user, &asset_id, b_tokens_minted, true)
                    .await;

                // Update reserve's estimated b rate by using request.amount/b_tokens_minted from the emitted event
                self.update_rate(pool_id, asset_id, supply_amount, b_tokens_minted, true)
            }
            "withdraw_collateral" => {
                let asset_id = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap(),
                );
                let user = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[2].as_bytes(), Limits::none()).unwrap(),
                );
                let withdraw_amount: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(0).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => (&amount).into(),
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };
                let b_tokens_burned: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(1).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => (&amount).into(),
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };
                println!(
                    "Withdraw Collateral!\n
                    asset address:{:?}\n
                    user address: {:?}\n
                    withdraw amount: {:?}\n
                    b tokens burned: {:?}",
                    asset_id, user, withdraw_amount, b_tokens_burned
                );
                // Update users collateral positions
                self.update_user(&pool_id, &user, &asset_id, -b_tokens_burned, true)
                    .await;

                // Update reserve estimated b rate by using tokens out/b tokens burned from the emitted event
                self.reserve_configs
                    .entry(pool_id)
                    .or_default()
                    .entry(asset_id)
                    .and_modify(|reserve_config| {
                        reserve_config.est_b_rate = withdraw_amount / b_tokens_burned;
                    });
            }
            "borrow" => {
                let asset_id = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap(),
                );
                let user = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[2].as_bytes(), Limits::none()).unwrap(),
                );
                let borrow_amount: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(0).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => (&amount).into(),
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };
                let d_token_minted: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(1).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => (&amount).into(),
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };
                println!(
                    "Borrow!\n
                    asset address:{:?}\n
                    user address: {:?}\n
                    borrow amount: {:?}\n
                    d tokens burned: {:?}",
                    asset_id, user, borrow_amount, d_token_minted
                );
                // Update users liability positions
                self.update_user(&pool_id, &user, &asset_id, d_token_minted, false)
                    .await;

                // Update reserve estimated b rate by using request.amount/d tokens minted from the emitted event
                self.update_rate(pool_id, asset_id, borrow_amount, d_token_minted, false)
            }
            "repay" => {
                let asset_id = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap(),
                );
                let user = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[2].as_bytes(), Limits::none()).unwrap(),
                );
                let repay_amount: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(0).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => (&amount).into(),
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };
                let d_token_burned: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(1).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => (&amount).into(),
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };
                println!(
                    "Repay!\n
                    asset address:{:?}\n
                    user address: {:?}\n
                    repay_amount: {:?}\n
                    d tokens burned: {:?}",
                    asset_id, user, repay_amount, d_token_burned
                );
                // Update users liability positions
                self.update_user(&pool_id, &user, &asset_id, -d_token_burned, false)
                    .await;
                // Update reserve estimated d rate by using request.amount/d tokens burnt from the emitted event
                self.update_rate(pool_id, asset_id, repay_amount, d_token_burned, false);
            }
            "oracle_update" => {
                // Update the asset price
                // TODO: idk these events will look like
                let asset_id = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap(),
                ); //TODO: placeholder

                // recalculate auction profitability
                for pending in self.pending_fill.iter_mut() {
                    let bids = pending.auction_data.bid.clone();
                    let lots = pending.auction_data.lot.clone();
                    if bids.contains_key(&asset_id) || lots.contains_key(&asset_id) {
                        match pending.auction_type {
                            0 => pending.calc_liquidation_fill(
                                &self.asset_prices,
                                self.reserve_configs.get(&pool_id).unwrap(),
                                self.bankroll.get(&pool_id).unwrap(),
                                self.min_hf,
                            ),
                            1 => pending.calc_bad_debt_fill(
                                &self.asset_prices,
                                self.reserve_configs.get(&pool_id).unwrap(),
                                self.bankroll.get(&self.us_public).unwrap(),
                                self.min_hf,
                                self.backstop_token_address.clone(),
                            ),
                            2 => pending.calc_interest_fill(
                                &self.asset_prices,
                                self.backstop_token_address.clone(),
                                self.wallet
                                    .get(&self.backstop_token_address)
                                    .unwrap()
                                    .clone(),
                            ),
                            _ => (),
                        }
                    }
                }
                // Check if we can liquidate anyone based on the new price
                for pool_reserves in self.reserve_configs.iter() {
                    if pool_reserves.1.contains_key(&asset_id) {
                        for users in self.users.get(pool_reserves.0).iter_mut() {
                            for user in users.iter() {
                                let score =
                                    evaluate_user(pool_reserves.1, &self.asset_prices, user.1);
                                // create liquidation auction if needed
                                if score > 1 {
                                    let op_builder = BlendTxBuilder {
                                        contract_id: pool_reserves.0.clone(),
                                        signing_key: self.us.clone(),
                                    };
                                    self.sequence_num += 1;
                                    let tx = op_builder
                                        .new_liquidation_auction(
                                            self.sequence_num,
                                            user.0.clone(),
                                            score,
                                        )
                                        .unwrap();
                                    actions.push(Action::SubmitTx(SubmitStellarTx {
                                        tx,
                                        gas_bid_info: None,
                                        signing_key: self.us.clone(),
                                    }));
                                }
                            }
                        }
                    }
                }
            }
            _ => (),
        }
        if actions.len() > 0 {
            return Some(actions.to_vec());
        }
        None::<Vec<Action>>
    }

    /// Process new block events, updating the internal state.
    async fn process_new_block_event(
        &mut self,
        event: NewBlock,
        actions: &mut Vec<Action>,
    ) -> Option<Vec<Action>> {
        let liquidator_id = Hash(self.us.verifying_key().to_bytes());
        for pending in self.pending_fill.iter() {
            let op_builder = BlendTxBuilder {
                contract_id: pending.pool.clone(),
                signing_key: self.us.clone(),
            };
            if pending.target_block <= event.number {
                self.sequence_num += 1;
                let tx = op_builder
                    .submit(
                        self.sequence_num,
                        liquidator_id.clone(),
                        liquidator_id.clone(),
                        liquidator_id.clone(),
                        vec![Request {
                            request_type: 6 + pending.auction_type,
                            address: pending.user.clone(),
                            amount: pending.pct_to_fill as i128,
                        }],
                    )
                    .unwrap();
                actions.push(Action::SubmitTx(SubmitStellarTx {
                    tx,
                    gas_bid_info: None,
                    signing_key: self.us.clone(),
                }));
            }
        }
        //TEMP: check if liquidations are possible every 50 blocks since we're not getting oracle update events atm
        if event.number % 50 == 0 {
            println!("checking for liqs");
            for pool_reserves in self.reserve_configs.iter() {
                for users in self.users.get(pool_reserves.0).iter_mut() {
                    for user in users.iter() {
                        let score = evaluate_user(pool_reserves.1, &self.asset_prices, user.1);
                        // create liquidation auction if needed
                        println!("score {}", score);
                        if score > 1 {
                            let op_builder = BlendTxBuilder {
                                contract_id: pool_reserves.0.clone(),
                                signing_key: self.us.clone(),
                            };
                            self.sequence_num += 1;
                            let tx = op_builder
                                .new_liquidation_auction(self.sequence_num, user.0.clone(), score)
                                .unwrap();
                            actions.push(Action::SubmitTx(SubmitStellarTx {
                                tx,
                                gas_bid_info: None,
                                signing_key: self.us.clone(),
                            }));
                        }
                    }
                }
            }
        }

        if actions.len() > 0 {
            return Some(actions.to_vec());
        }

        None
    }

    async fn get_user_position(&mut self, pool_id: Hash, user_id: Hash) -> Result<()> {
        let reserve_data_key = ScVal::Vec(Some(
            ScVec::try_from(vec![
                ScVal::Symbol(ScSymbol::from(ScSymbol::from(
                    StringM::from_str("Positions").unwrap(),
                ))),
                ScVal::Address(ScAddress::Account(AccountId(
                    PublicKey::PublicKeyTypeEd25519(Uint256(user_id.0)),
                ))),
            ])
            .unwrap(),
        ));
        let position_ledger_key =
            stellar_xdr::curr::LedgerKey::ContractData(LedgerKeyContractData {
                contract: ScAddress::Contract(pool_id.clone()),
                key: reserve_data_key,
                durability: stellar_xdr::curr::ContractDataDurability::Persistent,
            });
        let result = self
            .rpc
            .get_ledger_entries(&vec![position_ledger_key])
            .await
            .unwrap();
        if let Some(entries) = result.entries {
            for entry in entries {
                let value: LedgerEntryData =
                    LedgerEntryData::from_xdr_base64(entry.xdr, Limits::none()).unwrap();

                match &value {
                    LedgerEntryData::ContractData(data) => {
                        let mut user_id: Hash = Hash([0; 32]);
                        match &data.key {
                            ScVal::Vec(vec) => {
                                if let Some(vec) = vec {
                                    user_id = decode_scaddress_to_hash(&vec[1]);
                                } else {
                                    ();
                                }
                            }
                            _ => (),
                        }
                        let reserve_configs =
                            self.reserve_configs.entry(pool_id.clone()).or_default();
                        let user_position =
                            user_positions_from_ledger_entry(&value, &reserve_configs.to_owned());
                        println!("user: {:?}, {:?}", user_id, user_position.clone());
                        if user_id == self.us_public {
                            self.bankroll.insert(pool_id.clone(), user_position.clone());
                        } else {
                            let score = evaluate_user(
                                self.reserve_configs.get(&pool_id).unwrap(),
                                &self.asset_prices,
                                &user_position,
                            );
                            println!("score {}", score);
                            if score < 1
                                || !self.validate_assets(
                                    user_position.collateral.clone(),
                                    user_position.liabilities.clone(),
                                )
                            {
                                println!("removing user");
                                self.users
                                    .entry(pool_id.clone())
                                    .or_default()
                                    .remove(&user_id);
                            } else {
                                println!("adding user");
                                self.users
                                    .entry(pool_id.clone())
                                    .or_default()
                                    .insert(user_id.clone(), user_position);
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
        Ok(())
    }
    async fn get_asset_prices(&mut self, assets: Vec<Hash>) -> Result<()> {
        // A random key is fine for simulation
        let key = SigningKey::from_bytes(&[0; 32]);
        // get asset prices from oracle
        for asset in assets.iter() {
            let op = Operation {
                source_account: None,
                body: stellar_xdr::curr::OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                    host_function: stellar_xdr::curr::HostFunction::InvokeContract(
                        InvokeContractArgs {
                            contract_address: ScAddress::Contract(self.oracle_id.clone()),
                            function_name: ScSymbol::try_from("lastprice").unwrap(),
                            args: VecM::try_from(vec![ScVal::Vec(Some(
                                ScVec::try_from(vec![
                                    ScVal::Symbol(ScSymbol::try_from("Stellar").unwrap()),
                                    ScVal::Address(ScAddress::Contract(asset.clone())),
                                ])
                                .unwrap(),
                            ))])
                            .unwrap(),
                        },
                    ),
                    auth: VecM::default(),
                }),
            };
            let transaction: TransactionEnvelope = TransactionEnvelope::Tx(TransactionV1Envelope {
                tx: Transaction {
                    source_account: MuxedAccount::Ed25519(Uint256(key.verifying_key().to_bytes())),
                    fee: 10000,
                    seq_num: stellar_xdr::curr::SequenceNumber(10),
                    cond: Preconditions::None,
                    memo: Memo::None,
                    operations: vec![op].try_into()?,
                    ext: stellar_xdr::curr::TransactionExt::V0,
                },
                signatures: VecM::default(),
            });
            let sim_result = self.rpc.simulate_transaction(&transaction).await?;
            let contract_function_result =
                ScVal::from_xdr_base64(sim_result.results[0].xdr.clone(), Limits::none()).unwrap();
            let mut price: i128 = 0;
            match &contract_function_result {
                ScVal::Map(data_map) => {
                    if let Some(data_map) = data_map {
                        let entry = &data_map[0].val;
                        match entry {
                            ScVal::I128(value) => {
                                price = value.into();
                            }
                            _ => (),
                        }
                    }
                }
                _ => (),
            }
            //TODO: Decide whether we should scale down the price by the decimals
            self.asset_prices.insert(asset.clone(), price);
        }
        Ok(())
    }

    async fn get_reserve_config(&mut self, assets: Vec<Hash>) {
        for pool in &self.pools {
            let mut ledger_keys: Vec<LedgerKey> = Vec::new();
            for asset in &assets {
                let asset_id = ScVal::Address(ScAddress::Contract(asset.clone()));

                let reserve_config_key = ScVal::Vec(Some(
                    ScVec::try_from(vec![
                        ScVal::Symbol(ScSymbol::from(ScSymbol::from(
                            StringM::from_str("ResConfig").unwrap(),
                        ))),
                        asset_id.clone(),
                    ])
                    .unwrap(),
                ));
                let reserve_data_key = ScVal::Vec(Some(
                    ScVec::try_from(vec![
                        ScVal::Symbol(ScSymbol::from(ScSymbol::from(
                            StringM::from_str("ResData").unwrap(),
                        ))),
                        asset_id,
                    ])
                    .unwrap(),
                ));
                let reserve_config_ledger_key =
                    stellar_xdr::curr::LedgerKey::ContractData(LedgerKeyContractData {
                        contract: ScAddress::Contract(pool.clone()),
                        key: reserve_config_key,
                        durability: stellar_xdr::curr::ContractDataDurability::Persistent,
                    });
                let reserve_data_ledger_key =
                    stellar_xdr::curr::LedgerKey::ContractData(LedgerKeyContractData {
                        contract: ScAddress::Contract(pool.clone()),
                        key: reserve_data_key,
                        durability: stellar_xdr::curr::ContractDataDurability::Persistent,
                    });
                ledger_keys.push(reserve_config_ledger_key);
                ledger_keys.push(reserve_data_ledger_key);
            }

            let result = self.rpc.get_ledger_entries(&ledger_keys).await.unwrap();
            if let Some(entries) = result.entries {
                for entry in entries {
                    let value =
                        LedgerEntryData::from_xdr_base64(entry.xdr, Limits::none()).unwrap();
                    match &value {
                        LedgerEntryData::ContractData(data) => {
                            let key = decode_entry_key(&data.key);
                            let mut asset_id: Hash = Hash([0; 32]);
                            match &data.key {
                                ScVal::Vec(vec) => {
                                    if let Some(vec) = vec {
                                        asset_id = decode_scaddress_to_hash(&vec[1]);
                                    } else {
                                        ();
                                    }
                                }
                                _ => (),
                            }
                            match key.as_str() {
                                "ResData" => {
                                    let (b_rate, d_rate) = reserve_data_from_ledger_entry(&value);
                                    let config = self
                                        .reserve_configs
                                        .entry(pool.clone())
                                        .or_default()
                                        .entry(asset_id)
                                        .or_insert(ReserveConfig {
                                            index: 0,
                                            liability_factor: 0,
                                            collateral_factor: 0,
                                            scalar: 0,
                                            est_b_rate: b_rate,
                                            est_d_rate: d_rate,
                                        });
                                    config.est_b_rate = b_rate;
                                    config.est_d_rate = d_rate;
                                }
                                "ResConfig" => {
                                    let (index, collateral_factor, liability_factor, scalar) =
                                        reserve_config_from_ledger_entry(&value);
                                    println!("scalar {}", scalar);
                                    let config: &mut ReserveConfig = self
                                        .reserve_configs
                                        .entry(pool.clone())
                                        .or_default()
                                        .entry(asset_id)
                                        .or_insert(ReserveConfig {
                                            index,
                                            liability_factor,
                                            collateral_factor,
                                            scalar,
                                            est_b_rate: 0,
                                            est_d_rate: 0,
                                        });
                                    config.index = index;
                                    config.collateral_factor = collateral_factor;
                                    config.liability_factor = liability_factor;
                                }
                                _ => println!("Error: found unexpected key {}", key),
                            }
                        }
                        _ => (),
                    }
                }
            }
        }
    }
    fn update_rate(
        &mut self,
        pool_id: Hash,
        asset_id: Hash,
        numerator: i128,
        denominator: i128,
        b_rate: bool,
    ) {
        self.reserve_configs
            .entry(pool_id)
            .or_default()
            .entry(asset_id)
            .and_modify(|reserve_config| {
                if b_rate {
                    reserve_config.est_b_rate = numerator * 1e9 as i128 / denominator / 1e9 as i128;
                } else {
                    reserve_config.est_d_rate = numerator * 1e9 as i128 / denominator / 1e9 as i128;
                }
            });
    }
    // validates assets in two hashmaps of assets and amounts - common pattern
    fn validate_assets(&self, asset1: HashMap<Hash, i128>, asset2: HashMap<Hash, i128>) -> bool {
        for asset in asset1.keys().chain(asset2.keys()) {
            if !self.assets.contains(asset) {
                return false;
            }
        }
        return true;
    }

    async fn update_user(
        &mut self,
        pool_id: &Hash,
        user_id: &Hash,
        asset_id: &Hash,
        amount: i128,
        collateral: bool,
    ) {
        if !self.all_user.contains(user_id) {
            self.all_user.push(user_id.clone());
        } else if let Some(positions) = self.users.get_mut(&pool_id).unwrap().get_mut(&user_id) {
            if collateral {
                let balance = positions.collateral.entry(asset_id.clone()).or_insert(0);
                *balance += amount;
            } else {
                let balance = positions.liabilities.entry(asset_id.clone()).or_insert(0);
                *balance += amount;
            }
            // User's borrowing power is going up so we need to potentially drop them
            if ((collateral && amount > 0) || (!collateral && amount < 0))
                && (evaluate_user(
                    &self.reserve_configs.get(&pool_id).unwrap(),
                    &self.asset_prices,
                    &positions,
                ) < 1
                    || !self.assets.contains(asset_id))
            {
                self.users.get_mut(&pool_id).unwrap().remove(&user_id);
            }
        } else if (collateral && amount < 0) || (!collateral && amount > 0) {
            // User's borrowing power is going down so we should potentially add them
            self.get_user_position(pool_id.clone(), user_id.clone())
                .await
                .unwrap();
        }
    }
}
