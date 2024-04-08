use crate::auction_manager::OngoingAuction;
use anyhow::Result;
use artemis_core::executors::soroban_executor::GasBidInfo;
use artemis_core::{
    collectors::block_collector::NewBlock, executors::soroban_executor::SubmitStellarTx,
    types::Strategy,
};
use async_trait::async_trait;
use blend_utilities::constants::SCALAR_7;
use blend_utilities::helper::{
    bstop_token_to_usdc, db_path, decode_auction_data, decode_scaddress_to_hash, get_balance,
    get_single_asset_price, populate_db, user_positions_from_ledger_entry,
};
use blend_utilities::{
    transaction_builder::{BlendTxBuilder, Request},
    types::{Action, Config, Event, UserPositions},
};
use core::panic;
use ed25519_dalek::SigningKey;
use rusqlite::Connection;
use soroban_cli::utils::contract_id_from_str;
use soroban_fixed_point_math::FixedPoint;
use soroban_rpc::{Client, Event as SorobanEvent};
use soroban_spec_tools::from_string_primitive;
use std::{collections::HashMap, str::FromStr, vec};
use stellar_xdr::curr::{
    AccountId, Hash, LedgerEntryData, LedgerKeyContractData, Limits, PublicKey, ReadXdr, ScAddress,
    ScMap, ScMapEntry, ScSpecTypeDef, ScSymbol, ScVal, ScVec, StringM, Uint256, VecM,
};
use tracing::info;

pub struct BlendLiquidator {
    /// Soroban RPC client for interacting with chain
    rpc: Client,
    /// Assets we're interested in
    assets: Vec<Hash>,
    /// Vec of Blend pool addresses to bid on auctions in
    pools: Vec<Hash>,
    /// Backstop ID
    backstop_id: Hash,
    /// Amount of profits to bid in gas
    bid_percentage: u64,
    /// Required profitability for auctions
    required_profit: i128,
    /// Pending auction fills
    pending_fill: Vec<OngoingAuction>,
    /// Our positions
    bankroll: HashMap<Hash, UserPositions>,
    /// Our wallet
    wallet: HashMap<Hash, i128>,
    /// Our signing address
    us: SigningKey,
    /// Our public key
    pub us_public: Hash,
    // Our minimum health factor
    min_hf: i128,
    // Backstop token address
    pub backstop_token_address: Hash,
    // USDC token address
    usdc_address: Hash,
    // XLM address
    xlm_address: Hash,
}

impl BlendLiquidator {
    pub async fn new(config: &Config, signing_key: &SigningKey) -> Result<Self> {
        let client = Client::new(config.rpc_url.as_str())?;
        let db = Connection::open(db_path("blend_assets.db"))?;
        populate_db(&db, &config.assets)?;
        db.close().unwrap();
        Ok(Self {
            rpc: client,
            assets: config.assets.clone(),
            pools: config.pools.clone(),
            backstop_id: config.backstop.clone(),
            bid_percentage: config.bid_percentage,
            required_profit: config.required_profit,
            pending_fill: vec![],
            bankroll: HashMap::new(),
            wallet: HashMap::new(),
            us: signing_key.clone(),
            us_public: Hash(signing_key.verifying_key().as_bytes().clone()),
            min_hf: config.min_hf,
            backstop_token_address: config.backstop_token_address.clone(),
            usdc_address: config.usdc_token_address.clone(),
            xlm_address: config.xlm_address.clone(),
        })
    }
}

#[async_trait]
impl Strategy<Event, Action> for BlendLiquidator {
    async fn sync_state(&mut self) -> Result<()> {
        // Get our wallet assets
        for asset in self.assets.clone().iter() {
            match get_balance(&self.rpc, self.us_public.clone(), asset.clone(), false).await {
                Ok(balance) => {
                    self.wallet.insert(asset.clone(), balance);
                }
                Err(_) => {
                    self.wallet.insert(asset.clone(), 0);
                }
            }
        }

        match get_balance(
            &self.rpc,
            self.us_public.clone(),
            self.backstop_token_address.clone(),
            false,
        )
        .await
        {
            Ok(balance) => {
                self.wallet
                    .insert(self.backstop_token_address.clone(), balance);
            }
            Err(_) => {
                self.wallet.insert(self.backstop_token_address.clone(), 0);
            }
        }
        for pool in self.pools.clone() {
            // Get our positions
            self.get_our_position(pool.clone()).await.unwrap();

            // Get ongoing interest auctions
            self.get_interest_auction(pool.clone()).await?;
            // Get ongoing bad debt auctions
            self.get_bad_debt_auction(pool.clone()).await?;
        }
        // Get all liquidations ongoing
        let db = Connection::open(db_path("blend_users.db"))?;
        let last_row = 1000;
        for i in 1..last_row {
            let row = db.query_row("SELECT address FROM users WHERE id = ?1", [i], |row| {
                row.get::<_, String>(0)
            });
            let user = match row {
                Ok(user) => user,
                Err(_) => {
                    break;
                }
            };
            let user_hash = Hash(
                stellar_strkey::ed25519::PublicKey::from_string(&user)
                    .unwrap()
                    .0,
            );
            for pool in self.pools.clone() {
                self.get_user_liquidation(pool.clone(), user_hash.clone())
                    .await?;
            }
        }
        db.close().unwrap();

        info!("done syncing state");

        Ok(())
    }

    // Process incoming events, filter non-auction events, decide if we care about auctions
    async fn process_event(&mut self, event: Event) -> Vec<Action> {
        //
        let mut actions: Vec<Action> = [].to_vec();
        match event {
            Event::SorobanEvents(events) => {
                let events = *events;
                self.process_soroban_events(events, &mut actions).await;
            }
            Event::NewBlock(block) => {
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
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap(),
                );
                info!(
                    "New liquidation auction for user: {:?}",
                    PublicKey::PublicKeyTypeEd25519(Uint256(user.0.clone())).to_string()
                );

                let auction_data = decode_auction_data(data);

                if self.validate_assets(auction_data.lot.clone(), auction_data.bid.clone()) {
                    //update our positions
                    self.get_our_position(pool_id.clone()).await.unwrap();

                    let mut pending_fill = OngoingAuction::new(
                        pool_id.clone(),
                        user.clone(),
                        auction_data.clone(),
                        0,
                        self.required_profit,
                    );
                    pending_fill
                        .calc_liquidation_fill(self.bankroll.get(&pool_id).unwrap(), self.min_hf)
                        .unwrap();
                    info!(
                        " New pending fill for user: {:?}, block: {:?}",
                        PublicKey::PublicKeyTypeEd25519(Uint256(user.0.clone())).to_string(),
                        pending_fill.target_block.clone()
                    );
                    self.pending_fill.push(pending_fill);
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

                let mut pending_fill = OngoingAuction::new(
                    pool_id.clone(),
                    self.backstop_id.clone(),
                    auction_data.clone(),
                    auction_type,
                    self.required_profit,
                );
                //Bad debt auction
                // we only care about bid here
                if auction_type == 1
                    && self.validate_assets(auction_data.bid.clone(), HashMap::new())
                {
                    //update our positions
                    self.get_our_position(pool_id.clone()).await.unwrap();

                    pending_fill
                        .calc_bad_debt_fill(
                            self.bankroll.get(&pool_id).unwrap(),
                            self.min_hf,
                            bstop_token_to_usdc(
                                &self.rpc,
                                self.backstop_token_address.clone(),
                                self.backstop_id.clone(),
                                *pending_fill
                                    .auction_data
                                    .lot
                                    .get(&self.backstop_token_address)
                                    .unwrap(),
                                self.usdc_address.clone(),
                            )
                            .await
                            .unwrap(),
                        )
                        .unwrap();
                    info!(" New pending bad debt fill: {:?}", pending_fill.clone());
                    self.pending_fill.push(pending_fill);
                    //we only care about lot here
                } else if self.validate_assets(auction_data.lot.clone(), HashMap::new()) {
                    //update our wallet
                    match get_balance(
                        &self.rpc,
                        self.us_public.clone(),
                        self.backstop_token_address.clone(),
                        false,
                    )
                    .await
                    {
                        Ok(balance) => {
                            self.wallet
                                .insert(self.backstop_token_address.clone(), balance);
                        }
                        Err(_) => {
                            self.wallet.insert(self.backstop_token_address.clone(), 0);
                        }
                    }

                    //Interest auction
                    pending_fill
                        .calc_interest_fill(
                            self.wallet
                                .get(&self.backstop_token_address)
                                .unwrap()
                                .clone(),
                            self.backstop_token_address.clone(),
                            bstop_token_to_usdc(
                                &self.rpc,
                                self.backstop_token_address.clone(),
                                self.backstop_id.clone(),
                                *pending_fill
                                    .auction_data
                                    .bid
                                    .get(&self.backstop_token_address)
                                    .unwrap(),
                                self.usdc_address.clone(),
                            )
                            .await
                            .unwrap(),
                        )
                        .unwrap();
                    info!("New pending interest fill: {:?}", pending_fill.clone());
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
                info!(
                    "Auction filled, user: {:?}, liquidator {:?}",
                    PublicKey::PublicKeyTypeEd25519(Uint256(liquidated_id.0.clone())).to_string(),
                    PublicKey::PublicKeyTypeEd25519(Uint256(liquidator_id.0.clone())).to_string()
                );
                // if we filled update our bankroll
                if liquidator_id.0 == self.us_public.0 {
                    self.get_our_position(pool_id.clone()).await.unwrap();
                }

                for (index, pending_fill) in self.pending_fill.clone().iter_mut().enumerate() {
                    if pending_fill.user == liquidated_id
                        && pending_fill.pool == pool_id
                        && pending_fill.auction_type == auction_type
                    {
                        if fill_percentage == 100 {
                            self.pending_fill.remove(index);
                        } else {
                            pending_fill.partial_fill_update(fill_percentage as u64);
                        }
                        break;
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
        for pending in self.pending_fill.iter_mut() {
            // assess pending if we're within 50 blocks
            if pending.target_block as i128 - event.number as i128 <= 50 {
                let profit = match pending.auction_type {
                    0 => pending
                        .calc_liquidation_fill(
                            &self.bankroll.get(&pending.pool).unwrap(),
                            self.min_hf.clone(),
                        )
                        .unwrap(),
                    1 => pending
                        .calc_bad_debt_fill(
                            self.bankroll.get(&pending.pool).unwrap(),
                            self.min_hf,
                            bstop_token_to_usdc(
                                &self.rpc,
                                self.backstop_token_address.clone(),
                                self.backstop_id.clone(),
                                *pending
                                    .auction_data
                                    .lot
                                    .get(&self.backstop_token_address)
                                    .unwrap(),
                                self.usdc_address.clone(),
                            )
                            .await
                            .unwrap(),
                        )
                        .unwrap(),
                    2 => pending
                        .calc_interest_fill(
                            self.wallet
                                .get(&self.backstop_token_address)
                                .unwrap()
                                .clone(),
                            self.backstop_token_address.clone(),
                            bstop_token_to_usdc(
                                &self.rpc,
                                self.backstop_token_address.clone(),
                                self.backstop_id.clone(),
                                *pending
                                    .auction_data
                                    .bid
                                    .get(&self.backstop_token_address)
                                    .unwrap(),
                                self.usdc_address.clone(),
                            )
                            .await
                            .unwrap(),
                        )
                        .unwrap(),

                    _ => panic!("Invalid auction type"),
                };
                if pending.target_block <= event.number && profit > self.required_profit {
                    let op_builder = BlendTxBuilder {
                        contract_id: pending.pool.clone(),
                        signing_key: self.us.clone(),
                    };

                    let op = op_builder
                        .submit(
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
                        op,
                        gas_bid_info: Some(GasBidInfo {
                            total_profit: profit
                                .fixed_mul_floor(
                                    // We assume XLM price to be 15cents if it's not tracked by the oracle (you should track with oracle)
                                    get_single_asset_price(&self.xlm_address).unwrap_or(150_0000),
                                    SCALAR_7,
                                )
                                .unwrap(),

                            bid_percentage: self.bid_percentage,
                        }),
                        signing_key: self.us.clone(),
                    }));
                }
            }
        }

        if actions.len() > 0 {
            return Some(actions.to_vec());
        }

        None
    }

    async fn get_our_position(&mut self, pool_id: Hash) -> Result<()> {
        let reserve_data_key = ScVal::Vec(Some(
            ScVec::try_from(vec![
                ScVal::Symbol(ScSymbol::from(ScSymbol::from(
                    StringM::from_str("Positions").unwrap(),
                ))),
                ScVal::Address(ScAddress::Account(AccountId(
                    PublicKey::PublicKeyTypeEd25519(Uint256(self.us.verifying_key().to_bytes())),
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
                    LedgerEntryData::ContractData(_) => {
                        let user_position = user_positions_from_ledger_entry(&value, &pool_id)?;

                        self.bankroll.insert(pool_id.clone(), user_position.clone());
                    }
                    _ => (),
                }
            }
        }
        Ok(())
    }

    async fn get_user_liquidation(&mut self, pool: Hash, user: Hash) -> Result<()> {
        let pool_id = ScAddress::Contract(pool.clone());
        let reserve_data_key = ScVal::Vec(Some(
            ScVec::try_from(vec![
                ScVal::Symbol(ScSymbol::from(ScSymbol::from(
                    StringM::from_str("Auction").unwrap(),
                ))),
                ScVal::Map(Some(ScMap(
                    VecM::try_from(vec![
                        ScMapEntry {
                            key: from_string_primitive("auct_type", &ScSpecTypeDef::Symbol)
                                .unwrap(),
                            val: from_string_primitive("0", &ScSpecTypeDef::U32).unwrap(),
                        },
                        ScMapEntry {
                            key: from_string_primitive("user", &ScSpecTypeDef::Symbol).unwrap(),
                            val: ScVal::Address(ScAddress::Account(AccountId(
                                PublicKey::PublicKeyTypeEd25519(Uint256(user.0.clone())),
                            ))),
                        },
                    ])
                    .unwrap(),
                ))),
            ])
            .unwrap(),
        ));
        let position_ledger_key =
            stellar_xdr::curr::LedgerKey::ContractData(LedgerKeyContractData {
                contract: pool_id.clone(),
                key: reserve_data_key,
                durability: stellar_xdr::curr::ContractDataDurability::Temporary,
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
                        let auction_data = decode_auction_data(data.val.clone());
                        info!(
                            "Found outstanding user liquidation auction for: {:?}",
                            PublicKey::PublicKeyTypeEd25519(Uint256(user.0.clone())).to_string()
                        );
                        if self.validate_assets(auction_data.lot.clone(), auction_data.bid.clone())
                        {
                            let mut pending_fill = OngoingAuction::new(
                                pool.clone(),
                                user.clone(),
                                auction_data.clone(),
                                0,
                                self.required_profit,
                            );
                            pending_fill.calc_liquidation_fill(
                                self.bankroll.get(&pool).unwrap(),
                                self.min_hf,
                            )?;
                            info!(
                                " New pending fill for user: {:?}, block: {:?}",
                                PublicKey::PublicKeyTypeEd25519(Uint256(user.0.clone()))
                                    .to_string(),
                                pending_fill.target_block.clone()
                            );
                            self.pending_fill.push(pending_fill);
                        }
                    }
                    _ => (),
                }
            }
        }

        Ok(())
    }

    async fn get_bad_debt_auction(&mut self, pool: Hash) -> Result<()> {
        let pool_id = ScAddress::Contract(pool.clone());
        let reserve_data_key = ScVal::Vec(Some(
            ScVec::try_from(vec![
                ScVal::Symbol(ScSymbol::from(ScSymbol::from(
                    StringM::from_str("Auction").unwrap(),
                ))),
                ScVal::Map(Some(ScMap(
                    VecM::try_from(vec![
                        ScMapEntry {
                            key: from_string_primitive("auct_type", &ScSpecTypeDef::Symbol)
                                .unwrap(),
                            val: from_string_primitive("1", &ScSpecTypeDef::U32).unwrap(),
                        },
                        ScMapEntry {
                            key: from_string_primitive("user", &ScSpecTypeDef::Symbol).unwrap(),
                            val: ScVal::Address(ScAddress::Contract(self.backstop_id.clone())),
                        },
                    ])
                    .unwrap(),
                ))),
            ])
            .unwrap(),
        ));
        let position_ledger_key =
            stellar_xdr::curr::LedgerKey::ContractData(LedgerKeyContractData {
                contract: pool_id.clone(),
                key: reserve_data_key,
                durability: stellar_xdr::curr::ContractDataDurability::Temporary,
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
                        let auction_data = decode_auction_data(data.val.clone());
                        if self.validate_assets(auction_data.bid.clone(), HashMap::new()) {
                            let mut pending_fill = OngoingAuction::new(
                                pool.clone(),
                                self.backstop_id.clone(),
                                auction_data.clone(),
                                1,
                                self.required_profit,
                            );
                            let lot_value = bstop_token_to_usdc(
                                &self.rpc,
                                self.backstop_token_address.clone(),
                                self.backstop_id.clone(),
                                *pending_fill
                                    .auction_data
                                    .lot
                                    .get(&self.backstop_token_address)
                                    .unwrap(),
                                self.usdc_address.clone(),
                            )
                            .await
                            .unwrap();
                            pending_fill.calc_bad_debt_fill(
                                self.bankroll.get(&pool).unwrap(),
                                self.min_hf,
                                lot_value,
                            )?;
                            self.pending_fill.push(pending_fill);
                        }
                    }
                    _ => (),
                }
            }
        }

        Ok(())
    }

    async fn get_interest_auction(&mut self, pool: Hash) -> Result<()> {
        let pool_id = ScAddress::Contract(pool.clone());
        let reserve_data_key = ScVal::Vec(Some(
            ScVec::try_from(vec![
                ScVal::Symbol(ScSymbol::from(ScSymbol::from(
                    StringM::from_str("Auction").unwrap(),
                ))),
                ScVal::Map(Some(ScMap(
                    VecM::try_from(vec![
                        ScMapEntry {
                            key: from_string_primitive("auct_type", &ScSpecTypeDef::Symbol)
                                .unwrap(),
                            val: from_string_primitive("2", &ScSpecTypeDef::U32).unwrap(),
                        },
                        ScMapEntry {
                            key: from_string_primitive("user", &ScSpecTypeDef::Symbol).unwrap(),
                            val: ScVal::Address(ScAddress::Contract(self.backstop_id.clone())),
                        },
                    ])
                    .unwrap(),
                ))),
            ])
            .unwrap(),
        ));
        let position_ledger_key =
            stellar_xdr::curr::LedgerKey::ContractData(LedgerKeyContractData {
                contract: pool_id.clone(),
                key: reserve_data_key,
                durability: stellar_xdr::curr::ContractDataDurability::Temporary,
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
                        let auction_data = decode_auction_data(data.val.clone());
                        if self.validate_assets(auction_data.lot.clone(), HashMap::new()) {
                            let mut pending_fill = OngoingAuction::new(
                                pool.clone(),
                                self.backstop_id.clone(),
                                auction_data.clone(),
                                2,
                                self.required_profit,
                            );
                            let bid_value = bstop_token_to_usdc(
                                &self.rpc,
                                self.backstop_token_address.clone(),
                                self.backstop_id.clone(),
                                *pending_fill
                                    .auction_data
                                    .bid
                                    .get(&self.backstop_token_address)
                                    .unwrap(),
                                self.usdc_address.clone(),
                            )
                            .await
                            .unwrap();
                            pending_fill.calc_interest_fill(
                                self.wallet
                                    .get(&self.backstop_token_address)
                                    .unwrap()
                                    .clone(),
                                self.backstop_token_address.clone(),
                                bid_value,
                            )?;
                            self.pending_fill.push(pending_fill);
                        }
                    }
                    _ => (),
                }
            }
        }

        Ok(())
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
}
