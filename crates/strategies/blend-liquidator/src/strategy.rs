use std::collections::HashMap;
use std::str::FromStr;
use std::vec;
use artemis_core::collectors::block_collector::NewBlock;
use artemis_core::executors::soroban_executor::SubmitStellarTx;
use async_trait::async_trait;

use soroban_cli::utils::contract_id_from_str;
use stellar_strkey::ed25519::PrivateKey;
use stellar_xdr::curr::{
    ScAddress,
    Hash,
    ScVal,
    Limits,
    ReadXdr,
    LedgerKeyContractData,
    ScVec,
    VecM,
    ScSymbol,
    StringM,
    LedgerEntryData,
    TransactionEnvelope,
    TransactionV1Envelope,
    Transaction,
    Operation,
    InvokeHostFunctionOp,
    InvokeContractArgs,
    MuxedAccount,
    Uint256,
    Preconditions,
    Memo,
    AccountId,
    LedgerKey,
    PublicKey,
};
use ed25519_dalek::SigningKey;
use tracing::info;
use crate::constants::FACTORY_DEPLOYMENT_BLOCK;
use crate::transaction_builder::{ BlendTxBuilder, Request };
use crate::helper::{
    decode_scaddress_to_hash,
    decode_auction_data,
    reserve_config_from_ledger_entry,
    reserve_data_from_ledger_entry,
    user_positions_from_ledger_entry,
};

use crate::types::{ Config, UserPositions, ReserveConfig, AuctionData };
use anyhow::Result;
// use artemis_core::collectors::block_collector::NewBlock;
// use artemis_core::executors::soroban_executor::{ GasBidInfo, SubmitStellarTx };
use artemis_core::types::Strategy;
use soroban_cli::rpc::{ Client, Event as SorobanEvent };
use super::types::{ Action, Event, PendingFill };
use super::helper::decode_entry_key;
pub struct BlendLiquidator {
    /// Soroban RPC client for interacting with chain
    rpc: Client,

    /// The network url
    network_url: String,
    /// Assets we're interested in
    assets: Vec<Hash>,
    /// Map Assets to bid on and their prices
    asset_prices: HashMap<Hash, i128>,
    /// Vec of Blend pool addresses to bid on auctions in
    pools: Vec<Hash>,
    /// Oracle ID for getting asset prices
    oracle_id: Hash,
    /// Amount of profits to bid in gas
    bid_percentage: u64,
    /// Pending auction fills
    pending_fill: Vec<PendingFill>,
    /// Map of users and their positions in pools
    /// HashMap<UserId, HashMap<PoolId, UserPositions>>
    users: HashMap<Hash, HashMap<Hash, UserPositions>>,
    /// Map of pools and their reserve configurations
    reserve_configs: HashMap<Hash, HashMap<Hash, ReserveConfig>>,
    /// Our positions
    bankroll: HashMap<Hash, UserPositions>,
    /// Our address
    us: SigningKey,
    // Our minimum health factor
    min_hf: i128,
}

impl BlendLiquidator {
    pub fn new(config: Config) -> Self {
        Self {
            rpc: Client::new(config.rpc_url.as_str()).unwrap(),
            network_url: config.rpc_url,
            assets: config.assets,
            asset_prices: HashMap::new(),
            pools: config.pools,
            oracle_id: config.oracle_id,
            bid_percentage: config.bid_percentage,
            pending_fill: vec![],
            users: HashMap::new(),
            reserve_configs: HashMap::new(),
            bankroll: HashMap::new(),
            us: SigningKey::from_bytes(&PrivateKey::from_string(&config.us).unwrap().0),
            min_hf: config.min_hf,
        }
    }
}

#[async_trait]
impl Strategy<Event, Action> for BlendLiquidator {
    // In order to sync this strategy, we need to get the current bid for all Sudo pools.
    async fn sync_state(&mut self) -> Result<()> {
        // // Block in which the pool factory was deployed.
        let start_block = FACTORY_DEPLOYMENT_BLOCK;

        let current_block = self.rpc.get_latest_ledger().await?;

        // Get all asset prices
        self.get_asset_prices(self.assets.clone()).await?;

        // Get reserve configs for given pools
        self.get_reserve_config(self.assets.clone()).await;

        // Get user positions in given pools - also fill in our positions
        //TODO: decide if loading all user positions in one call is needed.
        self.get_user_position(
            self.pools[0].clone(),
            Hash(self.us.verifying_key().to_bytes())
        ).await?;
        info!("done syncing state, found available pools for {} collections", self.pools.len());
        Ok(())
    }

    // Process incoming events, filter non-auction events, decide if we care about auctions
    async fn process_event(&mut self, event: Event) -> Vec<Action> {
        //
        let mut actions: Vec<Action> = [].to_vec();
        match event {
            Event::SorobanEvents(events) => {
                let events = *events;
                if let Some(action) = self.process_soroban_events(events).await {
                }
            }
            Event::NewBlock(block) => {
                //TODO decide whether we need to execute a pending auction
                self.process_new_block_event(*block, &mut actions).await;
            }
        }
        actions
    }
}

impl BlendLiquidator {
    // Process new orders as they come in.
    async fn process_soroban_events(&mut self, event: SorobanEvent) -> Option<Vec<Action>> {
        let mut actions: Vec<Action> = Vec::default();

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

        //Deserialize event body cases
        match name.as_str() {
            "new_liquidation_auction" => {
                let user = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap()
                );

                let auction_data = decode_auction_data(data);
                // Check if assets being auctioned are in available assets
                let mut valid_assets = true;
                for asset in auction_data.collateral.keys() {
                    if !self.assets.contains(asset) {
                        valid_assets = false;
                    }
                }
                for asset in auction_data.liabilities.keys() {
                    if !self.assets.contains(asset) {
                        valid_assets = false;
                    }
                }
                if valid_assets {
                    self.create_pending_fill(pool_id, user, auction_data, false);
                }
                // TODO
                // Decide when to fill this auction based on our required profitability
                // Decide how much of this auction to fill based on the amount of assets we can support given pool positions
                // If yes add to a list of pending auctions (pct filled 0%)
            }
            "delete_liquidation_auction" => {
                // If this was an auction we were planning on filling, remove it from the pending list
                let user = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap()
                );
                for (index, pending_fill) in self.pending_fill.clone().iter().enumerate() {
                    if pending_fill.user.0 == user.0 {
                        let _ = &self.pending_fill.remove(index);
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

                //Check if auctioned assets are in available assets
                let mut valid_assets = true;
                for asset in auction_data.collateral.keys() {
                    if !self.assets.contains(asset) {
                        valid_assets = false;
                    }
                }
                for asset in auction_data.liabilities.keys() {
                    if !self.assets.contains(asset) {
                        valid_assets = false;
                    }
                }
                if valid_assets {
                    //Bad debt auction
                    if auction_type == 1 {
                        //TODO
                        // If a bad debt auction
                        // Decide when to fill this auction based on our required profitability
                        // Decide how much of this auction to fill based on the amount of assets we can support given pool positions
                        // If yes add to a list of pending auctions (pct filled 0%)
                    } else {
                        //TODO
                        //Interest Auction
                        // If an interest auction
                        // Decide when to fill this auction based on our required profitability
                        // Decide how much of this auction to fill based on how much USDC we have/can source - we will attempt to borrow the necessary USDC from the pool holding the auction (this just keeps the bot simple)
                    }
                }
            }
            "fill_auction" => {
                let liquidated_id = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap()
                );
                let fill_percentage: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(1).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => { (&amount).into() }
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };

                for (index, pending_fill) in self.pending_fill.clone().iter().enumerate() {
                    if pending_fill.user == liquidated_id && pending_fill.pool == pool_id {
                        if fill_percentage == 100 {
                            self.pending_fill.remove(index);
                        } else {
                            //Update pct_filled for pending fill
                            let old_fill_percent = pending_fill.pct_filled;
                            self.pending_fill[index].pct_filled =
                                old_fill_percent +
                                (1 - old_fill_percent) * (fill_percentage as u64);

                            // also update the percent that we were planning on filling
                            // Note: we gotta differentiate btw backstop and interest auctions here
                            // we should check whether bad debt was created if the auction was filled post 200 blocks and if so attempt to move bad debt to the backstop and see if we can liquidate it - may be uneccesary
                        }
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
                self.get_reserve_config(vec![asset_id]).await;
            }
            "supply" => {
                let asset_id = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap()
                );

                let b_tokens_minted: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(1).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => { (&amount).into() }
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
                                ScVal::I128(amount) => { (&amount).into() }
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };

                self.reserve_configs
                    .entry(pool_id)
                    .or_default()
                    .entry(asset_id)
                    .and_modify(|reserve_config| {
                        reserve_config.est_b_rate = supply_amount / b_tokens_minted;
                    });
                // Update reserve estimated b rate by using request.amount/b_tokens_minted from the emitted event
            }
            "withdraw" => {
                let asset_id = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap()
                );
                let b_tokens_burned: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(1).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => { (&amount).into() }
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
                                ScVal::I128(amount) => { (&amount).into() }
                                _ => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => 0,
                };

                self.reserve_configs
                    .entry(pool_id)
                    .or_default()
                    .entry(asset_id)
                    .and_modify(|reserve_config| {
                        reserve_config.est_b_rate = withdraw_amount / b_tokens_burned;
                    });
                // Update reserve estimated b rate by using tokens out/b tokens burned from the emitted event
            }
            "supply_collateral" => {
                let asset_id = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap()
                );
                let user = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[2].as_bytes(), Limits::none()).unwrap()
                );
                let supply_amount: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(0).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => { (&amount).into() }
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
                                ScVal::I128(amount) => { (&amount).into() }
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

                // Update user's collateral position
                let balance = self.users
                    .entry(user.clone())
                    .or_default()
                    .entry(pool_id.clone())
                    .or_insert(UserPositions {
                        collateral: Default::default(),
                        liabilities: Default::default(),
                    })
                    .collateral.entry(asset_id.clone())
                    .or_insert(0);
                *balance += supply_amount;
                // Update reserve's estimated b rate by using request.amount/b_tokens_minted from the emitted event
                self.reserve_configs
                    .entry(pool_id)
                    .or_default()
                    .entry(asset_id)
                    .and_modify(|reserve_config| {
                        reserve_config.est_b_rate = supply_amount / b_tokens_minted;
                    });
            }
            "withdraw_collateral" => {
                let asset_id = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap()
                );
                let user = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[2].as_bytes(), Limits::none()).unwrap()
                );
                let withdraw_amount: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(0).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => { (&amount).into() }
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
                                ScVal::I128(amount) => { (&amount).into() }
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
                    asset_id,
                    user,
                    withdraw_amount,
                    b_tokens_burned
                );
                // Update users collateral positions
                let balance = self.users
                    .entry(user.clone())
                    .or_default()
                    .entry(pool_id.clone())
                    .or_insert(UserPositions {
                        collateral: Default::default(),
                        liabilities: Default::default(),
                    })
                    .collateral.entry(asset_id.clone())
                    .or_insert(0);
                *balance -= withdraw_amount;
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
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap()
                );
                let user = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[2].as_bytes(), Limits::none()).unwrap()
                );
                let borrow_amount: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(0).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => { (&amount).into() }
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
                                ScVal::I128(amount) => { (&amount).into() }
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
                    asset_id,
                    user,
                    borrow_amount,
                    d_token_minted
                );
                // Update users liability positions
                let balance = self.users
                    .entry(user.clone())
                    .or_default()
                    .entry(pool_id.clone())
                    .or_insert(UserPositions {
                        collateral: Default::default(),
                        liabilities: Default::default(),
                    })
                    .liabilities.entry(asset_id.clone())
                    .or_insert(0);
                *balance += borrow_amount;
                // Update reserve estimated b rate by using request.amount/d tokens minted from the emitted event
                self.reserve_configs
                    .entry(pool_id)
                    .or_default()
                    .entry(asset_id)
                    .and_modify(|reserve_config| {
                        reserve_config.est_b_rate = borrow_amount / d_token_minted;
                    });
            }
            "repay" => {
                let asset_id = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap()
                );
                let user = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[2].as_bytes(), Limits::none()).unwrap()
                );
                let repay_amount: i128 = match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let amount = vec.clone().get(0).unwrap().to_owned();
                            match amount {
                                ScVal::I128(amount) => { (&amount).into() }
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
                                ScVal::I128(amount) => { (&amount).into() }
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
                    asset_id,
                    user,
                    repay_amount,
                    d_token_burned
                );
                // Update users liability positions
                let balance = self.users
                    .entry(user.clone())
                    .or_default()
                    .entry(pool_id.clone())
                    .or_insert(UserPositions {
                        collateral: Default::default(),
                        liabilities: Default::default(),
                    })
                    .liabilities.entry(asset_id.clone())
                    .or_insert(0);
                *balance -= repay_amount;
                // Update reserve estimated d rate by using request.amount/d tokens burnt from the emitted event
                self.reserve_configs
                    .entry(pool_id)
                    .or_default()
                    .entry(asset_id)
                    .and_modify(|reserve_config| {
                        reserve_config.est_b_rate = repay_amount / d_token_burned;
                    });
            }
            "oracle_update" => {
                todo!();
                // Update the asset price
                // Check if we can liquidate anyone based on the new price
                // If we can, create an auction
                // We probably need to re-assess auction profitability here
            }
            _ => (),
        }

        None::<Vec<Action>>
        // Build arb tx.
        // self.build_arb_tx(event.listing.order_hash, *max_pool, *max_bid)
        //     .await
    }

    /// Process new block events, updating the internal state.
    async fn process_new_block_event(
        &mut self,
        event: NewBlock,
        actions: &mut Vec<Action>
    ) -> Option<Vec<Action>> {
        let tx_builder = BlendTxBuilder {
            rpc: Client::new(&self.network_url).unwrap(),
        };
        let liquidator_id = Hash(self.us.verifying_key().to_bytes());
        for pending in self.pending_fill.iter() {
            if pending.target_block <= event.number {
                // TODO: Create a fill tx
                let tx = tx_builder
                    .submit(
                        pending.pool.clone(),
                        liquidator_id.clone(),
                        liquidator_id.clone(),
                        liquidator_id.clone(),
                        vec![Request {
                            request_type: 6,
                            address: pending.user.clone(),
                            amount: 100,
                        }],
                        self.us.clone()
                    ).await
                    .unwrap();
                actions.push(Action::SubmitTx(SubmitStellarTx { tx, gas_bid_info: None }));
            }
        }
        if actions.len() > 0 {
            return Some(actions.to_vec());
        }

        None
    }

    async fn get_user_position(&mut self, pool_id: Hash, user_id: Hash) -> Result<()> {
        let reserve_data_key = ScVal::Vec(
            Some(
                ScVec::try_from(
                    vec![
                        ScVal::Symbol(
                            ScSymbol::from(ScSymbol::from(StringM::from_str("Positions").unwrap()))
                        ),
                        ScVal::Address(
                            ScAddress::Account(
                                AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(user_id.0)))
                            )
                        )
                    ]
                ).unwrap()
            )
        );
        let position_ledger_key = stellar_xdr::curr::LedgerKey::ContractData(LedgerKeyContractData {
            contract: ScAddress::Contract(pool_id.clone()),
            key: reserve_data_key,
            durability: stellar_xdr::curr::ContractDataDurability::Persistent,
        });
        let result = self.rpc.get_ledger_entries(&vec![position_ledger_key]).await.unwrap();
        if let Some(entries) = result.entries {
            for entry in entries {
                let value: LedgerEntryData = LedgerEntryData::from_xdr_base64(
                    entry.xdr,
                    Limits::none()
                ).unwrap();

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
                        let reserve_configs = self.reserve_configs
                            .entry(pool_id.clone())
                            .or_default();
                        let user_position = user_positions_from_ledger_entry(
                            &value,
                            &reserve_configs.to_owned()
                        );
                        println!("{:?}", user_position.clone());
                        self.users
                            .entry(user_id)
                            .or_default()
                            .insert(pool_id.clone(), user_position);
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
                            args: VecM::try_from(
                                vec![
                                    ScVal::Vec(
                                        Some(
                                            ScVec::try_from(
                                                vec![
                                                    ScVal::Symbol(
                                                        ScSymbol::try_from("Stellar").unwrap()
                                                    ),
                                                    ScVal::Address(
                                                        ScAddress::Contract(asset.clone())
                                                    )
                                                ]
                                            ).unwrap()
                                        )
                                    )
                                ]
                            ).unwrap(),
                        }
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
            let contract_function_result = ScVal::from_xdr_base64(
                sim_result.results[0].xdr.clone(),
                Limits::none()
            ).unwrap();
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

                let reserve_config_key = ScVal::Vec(
                    Some(
                        ScVec::try_from(
                            vec![
                                ScVal::Symbol(
                                    ScSymbol::from(
                                        ScSymbol::from(StringM::from_str("ResConfig").unwrap())
                                    )
                                ),
                                asset_id.clone()
                            ]
                        ).unwrap()
                    )
                );
                let reserve_data_key = ScVal::Vec(
                    Some(
                        ScVec::try_from(
                            vec![
                                ScVal::Symbol(
                                    ScSymbol::from(
                                        ScSymbol::from(StringM::from_str("ResData").unwrap())
                                    )
                                ),
                                asset_id
                            ]
                        ).unwrap()
                    )
                );
                let reserve_config_ledger_key = stellar_xdr::curr::LedgerKey::ContractData(
                    LedgerKeyContractData {
                        contract: ScAddress::Contract(pool.clone()),
                        key: reserve_config_key,
                        durability: stellar_xdr::curr::ContractDataDurability::Persistent,
                    }
                );
                let reserve_data_ledger_key = stellar_xdr::curr::LedgerKey::ContractData(
                    LedgerKeyContractData {
                        contract: ScAddress::Contract(pool.clone()),
                        key: reserve_data_key,
                        durability: stellar_xdr::curr::ContractDataDurability::Persistent,
                    }
                );
                ledger_keys.push(reserve_config_ledger_key);
                ledger_keys.push(reserve_data_ledger_key);
            }

            let result = self.rpc.get_ledger_entries(&ledger_keys).await.unwrap();
            if let Some(entries) = result.entries {
                for entry in entries {
                    let value = LedgerEntryData::from_xdr_base64(
                        entry.xdr,
                        Limits::none()
                    ).unwrap();
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
                                    let config = self.reserve_configs
                                        .entry(pool.clone())
                                        .or_default()
                                        .entry(asset_id)
                                        .or_insert(ReserveConfig {
                                            index: 0,
                                            liability_factor: 0,
                                            collateral_factor: 0,
                                            est_b_rate: b_rate,
                                            est_d_rate: d_rate,
                                        });
                                    config.est_b_rate = b_rate;
                                    config.est_d_rate = d_rate;
                                }
                                "ResConfig" => {
                                    let (index, collateral_factor, liability_factor) =
                                        reserve_config_from_ledger_entry(&value);
                                    let config = self.reserve_configs
                                        .entry(pool.clone())
                                        .or_default()
                                        .entry(asset_id)
                                        .or_insert(ReserveConfig {
                                            index,
                                            liability_factor,
                                            collateral_factor,
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

    async fn update_user_positions(&mut self, pool: Hash) {
        //TODO
        // we need to use the client to grab all users and store their positions - also needs to recognize us and store our position data in the bankroll
    }

    fn check_health(&self, user: Hash) -> bool {
        //TODO
        // Check user health return true if healthy
        true
    }

    fn create_pending_fill(
        &mut self,
        pool: Hash,
        user: Hash,
        auction_data: AuctionData,
        interest_auction: bool
    ) {
        //TODO
        // Create a pending fill and add it to the pending fill list
    }
}
