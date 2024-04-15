use anyhow::Result;
use artemis_core::{
    collectors::block_collector::NewBlock, executors::soroban_executor::SubmitStellarTx,
    types::Strategy,
};
use soroban_spec_tools::from_string_primitive;

use crate::{
    db_manager::DbManager,
    helper::{
        decode_scaddress_to_hash, evaluate_user, get_asset_prices_db, get_reserve_config_db,
        update_rate, user_positions_from_ledger_entry,
    },
    transaction_builder::BlendTxBuilder,
    types::{Action, Config, Event, UserPositions},
};
use async_trait::async_trait;
use ed25519_dalek::SigningKey;
use soroban_cli::utils::contract_id_from_str;
use soroban_rpc::{Client, Event as SorobanEvent};
use std::{collections::HashMap, str::FromStr, vec};
use stellar_xdr::curr::{
    AccountId, Hash, LedgerEntryData, LedgerKeyContractData, Limits, PublicKey, ReadXdr, ScAddress,
    ScMap, ScMapEntry, ScSpecTypeDef, ScSymbol, ScVal, ScVec, StringM, Uint256, VecM,
};
use tracing::info;

pub struct BlendAuctioneer {
    /// Soroban RPC client for interacting with chain
    rpc: Client,
    /// The path to the database directory
    db_manager: DbManager,
    /// Assets in pools
    assets: Vec<Hash>,
    /// Vec of Blend pool addresses to create auctions for
    pools: Vec<Hash>,
    /// Map pool users and their positions
    /// - only stores users with health factor < 5
    /// - only stores users with relevant assets
    /// HashMap<PoolId, HashMap<UserId, UserPositions>>
    users: HashMap<Hash, HashMap<Hash, UserPositions>>,
    /// Our signing address
    us: SigningKey,
    /// Our public key
    pub us_public: Hash,
    // Backstop token address
    pub backstop_token_address: Hash,
    // Oracle id
    oracle_id: Hash,
    // Oracle Decimals
    oracle_decimals: u32,
}

impl BlendAuctioneer {
    pub async fn new(config: &Config, signing_key: &SigningKey) -> Result<Self> {
        let client = Client::new(config.rpc_url.as_str())?;
        let db_manager = DbManager::new(config.db_path.clone());
        db_manager.initialize(&config.assets)?;

        get_asset_prices_db(
            &client,
            &config.oracle_id,
            &config.oracle_decimals,
            &config.assets,
            &db_manager,
        )
        .await?;
        get_reserve_config_db(&client, &config.pools, &config.assets, &db_manager).await?;
        Ok(Self {
            rpc: client,
            db_manager: DbManager::new(config.db_path.clone()),
            assets: config.assets.clone(),
            pools: config.pools.clone(),
            users: HashMap::new(),
            us: signing_key.clone(),
            us_public: Hash(signing_key.verifying_key().as_bytes().clone()),
            backstop_token_address: config.backstop_token_address.clone(),
            oracle_id: config.oracle_id.clone(),
            oracle_decimals: config.oracle_decimals,
        })
    }
}

#[async_trait]
impl Strategy<Event, Action> for BlendAuctioneer {
    async fn sync_state(&mut self) -> Result<()> {
        let users = self.db_manager.get_users()?;
        for user in users {
            for pool in self.pools.clone() {
                if self
                    .user_has_liquidation(pool.clone(), user.clone())
                    .await?
                {
                    continue;
                }
                self.get_user_position(pool.clone(), user.clone()).await?;
            }
        }
        info!("synced auctioneer state");
        Ok(())
    }

    // Process incoming events
    async fn process_event(&mut self, event: Event) -> Vec<Action> {
        let mut actions: Vec<Action> = [].to_vec();

        match event {
            Event::SorobanEvents(events) => {
                let events = *events;
                let mut retry_counter = 0;
                while retry_counter < 100 {
                    let result = self
                        .process_soroban_events(events, &mut actions, retry_counter)
                        .await;
                    match result {
                        Ok(actions) => return actions,
                        Err(e) => {
                            retry_counter += 1;
                            info!("retrying soroban event processing");
                        }
                    }
                }
            }
            Event::NewBlock(block) => {
                self.process_new_block_event(*block, &mut actions).await;
            }
        }
        actions
    }
}

impl BlendAuctioneer {
    // Process new orders as they come in.
    async fn process_soroban_events(
        &mut self,
        event: SorobanEvent,
        actions: &mut Vec<Action>,
        retry_count: u32,
    ) -> Result<Vec<Action>> {
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

                // remove user from users list since they are being liquidated
                self.users.entry(pool_id.clone()).or_default().remove(&user);
            }
            "delete_liquidation_auction" => {
                // If this was an auction we were planning on filling, remove it from the pending list
                let user = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap(),
                );

                // add user back to users
                self.get_user_position(pool_id.clone(), user.clone())
                    .await
                    .unwrap();
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

                if fill_percentage == 100 && auction_type == 0 {
                    // add user back to positions
                    let score = self
                        .get_user_position(pool_id.clone(), liquidated_id.clone())
                        .await
                        .unwrap();
                    //check if a bad debt call is necessary
                    if score.is_some() && score.unwrap() != 1 {
                        let action = self.act_on_score(&liquidated_id, &pool_id, score.unwrap());
                        if action.is_some() {
                            actions.push(action.unwrap());
                        }
                    }
                }
            }
            "bad_debt" => {
                let user = decode_scaddress_to_hash(
                    &ScVal::from_xdr_base64(event.topic[1].as_bytes(), Limits::none()).unwrap(),
                );
                // remove user from users list since their positions were removed
                self.users.entry(pool_id.clone()).or_default().remove(&user);
                let tx_builder = BlendTxBuilder {
                    contract_id: pool_id.clone(),
                    signing_key: self.us.clone(),
                };
                actions.push(Action::SubmitTx(SubmitStellarTx {
                    op: tx_builder.new_bad_debt_auction().unwrap(),
                    gas_bid_info: None,
                    signing_key: self.us.clone(),
                }));
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
                get_reserve_config_db(&self.rpc, &vec![pool_id], &vec![asset_id], &self.db_manager)
                    .await
                    .unwrap();
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
                if supply_amount == 0 || b_tokens_minted == 0 {
                    return Ok(None::<Vec<Action>>);
                }
                // Update reserve estimated b rate by using request.amount/b_tokens_minted from the emitted event
                let new_rate = update_rate(supply_amount, b_tokens_minted);
                match new_rate {
                    Ok(rate) => {
                        self.db_manager
                            .update_reserve_config_rate(&pool_id, &asset_id, rate, true)
                            .unwrap();
                    }
                    Err(_) => get_reserve_config_db(
                        &self.rpc,
                        &vec![pool_id],
                        &vec![asset_id],
                        &self.db_manager,
                    )
                    .await
                    .unwrap(),
                }
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
                if withdraw_amount == 0 || b_tokens_burned == 0 {
                    return None::<Vec<Action>>;
                }
                // Update reserve estimated b rate by using tokens out/b tokens burned from the emitted event
                let new_rate = update_rate(withdraw_amount, b_tokens_burned);
                match new_rate {
                    Ok(rate) => {
                        self.db_manager
                            .update_reserve_config_rate(&pool_id, &asset_id, rate, true)
                            .unwrap();
                    }
                    Err(_) => get_reserve_config_db(
                        &self.rpc,
                        &vec![pool_id],
                        &vec![asset_id],
                        &self.db_manager,
                    )
                    .await
                    .unwrap(),
                }
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

                if supply_amount == 0 || b_tokens_minted == 0 {
                    return None::<Vec<Action>>;
                }
                self.update_user(&pool_id, &user, &asset_id, b_tokens_minted, true)
                    .await
                    .unwrap();

                // Update reserve's estimated b rate by using request.amount/b_tokens_minted from the emitted event

                let new_rate = update_rate(supply_amount, b_tokens_minted);
                match new_rate {
                    Ok(rate) => {
                        self.db_manager
                            .update_reserve_config_rate(&pool_id, &asset_id, rate, true)
                            .unwrap();
                    }
                    Err(_) => get_reserve_config_db(
                        &self.rpc,
                        &vec![pool_id],
                        &vec![asset_id],
                        &self.db_manager,
                    )
                    .await
                    .unwrap(),
                }
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

                if withdraw_amount == 0 || b_tokens_burned == 0 {
                    return None::<Vec<Action>>;
                }
                // Update users collateral positions
                self.update_user(&pool_id, &user, &asset_id, -b_tokens_burned, true)
                    .await
                    .unwrap();

                // Update reserve estimated b rate by using tokens out/b tokens burned from the emitted event

                let new_rate = update_rate(withdraw_amount, b_tokens_burned);
                match new_rate {
                    Ok(rate) => {
                        self.db_manager
                            .update_reserve_config_rate(&pool_id, &asset_id, rate, true)
                            .unwrap();
                    }
                    Err(_) => get_reserve_config_db(
                        &self.rpc,
                        &vec![pool_id],
                        &vec![asset_id],
                        &self.db_manager,
                    )
                    .await
                    .unwrap(),
                }
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
                if borrow_amount == 0 || d_token_burned == 0 {
                    return None::<Vec<Action>>;
                }

                // Update users liability positions
                self.update_user(&pool_id, &user, &asset_id, d_token_burned, false)
                    .await
                    .unwrap();

                // Update reserve estimated b rate by using request.amount/d tokens minted from the emitted event
                let new_rate = update_rate(borrow_amount, d_token_burned);
                match new_rate {
                    Ok(rate) => {
                        self.db_manager
                            .update_reserve_config_rate(&pool_id, &asset_id, rate, true)
                            .unwrap();
                    }
                    Err(_) => get_reserve_config_db(
                        &self.rpc,
                        &vec![pool_id],
                        &vec![asset_id],
                        &self.db_manager,
                    )
                    .await
                    .unwrap(),
                }
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

                if repay_amount == 0 || d_token_burned == 0 {
                    return Ok(None::<Vec<Action>>);
                }
                // Update users liability positions
                self.update_user(&pool_id, &user, &asset_id, -d_token_burned, false)
                    .await
                    .unwrap();
                // Update reserve estimated d rate by using request.amount/d tokens burnt from the emitted event

                let new_rate = update_rate(repay_amount, d_token_burned);
                match new_rate {
                    Ok(rate) => {
                        self.db_manager
                            .update_reserve_config_rate(&pool_id, &asset_id, rate, true)
                            .unwrap();
                    }
                    Err(_) => get_reserve_config_db(
                        &self.rpc,
                        &vec![pool_id],
                        &vec![asset_id],
                        &self.db_manager,
                    )
                    .await
                    .unwrap(),
                }
            }
            //if oracle has events they can be handled here
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
        //TEMP: check if liquidations are possible every 100 blocks since we're not getting oracle update events atm
        if event.number % 100 == 0 {
            info!("on block: {} ", event.number);
        }
        if event.number % 10 == 0 {
            println!("Should be checking users on block: {} ", event.number);
            get_asset_prices_db(
                &self.rpc,
                &self.oracle_id,
                &self.oracle_decimals,
                &self.assets,
                &self.db_manager,
            )
            .await
            .unwrap();
            for pool in self.pools.iter() {
                println!("checking pool: {:?}", pool);
                for users in self.users.get(pool).iter_mut() {
                    println!("{:?}", users);
                    for user in users.iter() {
                        println!("{:?}", user);
                        let score = evaluate_user(pool, user.1, &self.db_manager).unwrap();
                        // create liquidation auction if needed
                        let action = self.act_on_score(&user.0, &pool, score);
                        if action.is_some() {
                            info!(
                                "Creating liquidation auction for user: {:?}",
                                PublicKey::PublicKeyTypeEd25519(Uint256(user.0 .0)).to_string()
                            );
                            actions.push(action.unwrap());
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

    async fn get_user_position(&mut self, pool_id: Hash, user_id: Hash) -> Result<Option<u64>> {
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
                        let user_position =
                            user_positions_from_ledger_entry(&value, &pool_id, &self.db_manager)?;

                        let score = evaluate_user(&pool_id, &user_position, &self.db_manager)?;
                        if score != 1 {
                            self.users
                                .entry(pool_id.clone())
                                .or_default()
                                .insert(user_id.clone(), user_position);
                        }
                        return Ok(Some(score));
                    }
                    _ => (),
                }
            }
        }
        Ok(None)
    }
    async fn user_has_liquidation(&mut self, pool: Hash, user: Hash) -> Result<bool> {
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
                    LedgerEntryData::ContractData(_) => {
                        info!(
                            "Found outstanding user liquidation auction for: {:?}",
                            PublicKey::PublicKeyTypeEd25519(Uint256(user.0.clone())).to_string()
                        );
                        return Ok(true);
                    }
                    _ => (),
                }
            }
        }

        Ok(false)
    }

    fn act_on_score(&self, user: &Hash, pool: &Hash, score: u64) -> Option<Action> {
        let tx_builder = BlendTxBuilder {
            contract_id: pool.clone(),
            signing_key: self.us.clone(),
        };
        if score == 0 {
            // Code to execute if the value is None
            return Some(Action::SubmitTx(SubmitStellarTx {
                op: tx_builder.bad_debt(user.clone()).unwrap(),
                gas_bid_info: None,
                signing_key: self.us.clone(),
            }));
        }

        if score > 2 {
            // Code to execute if the value is None
            return Some(Action::SubmitTx(SubmitStellarTx {
                op: tx_builder
                    .new_liquidation_auction(user.clone(), score)
                    .unwrap(),
                gas_bid_info: None,
                signing_key: self.us.clone(),
            }));
        }
        None
    }

    // Updates user positions based on action
    // - If we do not have the user tracked we add them
    // - If we do have them tracked and they are adding an unsupported asset or their score is 1 we remove them
    async fn update_user(
        &mut self,
        pool_id: &Hash,
        user_id: &Hash,
        asset_id: &Hash,
        amount: i128,
        collateral: bool,
    ) -> Result<()> {
        self.db_manager.add_user(user_id)?;
        let pool = self.users.entry(pool_id.clone()).or_default();
        if let Some(positions) = pool.get_mut(&user_id) {
            if collateral {
                let balance = positions.collateral.entry(asset_id.clone()).or_insert(0);
                *balance += amount;
            } else {
                let balance = positions.liabilities.entry(asset_id.clone()).or_insert(0);
                *balance += amount;
            }

            // user's borrowing power is going up so we should potentially drop them
            if (collateral && amount > 0) || (!collateral && amount < 0) {
                let score = evaluate_user(&pool_id, &positions, &self.db_manager)?;
                if score == 1 {
                    pool.remove(&user_id);
                }
            }
        } else if (collateral && amount < 0) || (!collateral && amount > 0) {
            // User's borrowing power is going down so we should potentially add them
            self.get_user_position(pool_id.clone(), user_id.clone())
                .await
                .unwrap();
        }
        Ok(())
    }
}
