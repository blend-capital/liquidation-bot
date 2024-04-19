use anyhow::Result;
use artemis_core::{
    collectors::block_collector::NewBlock, executors::soroban_executor::SubmitStellarTx,
    types::Strategy,
};
use soroban_spec_tools::from_string_primitive;

use crate::{
    db_manager::DbManager,
    error_logger::log_error,
    helper::{
        decode_scaddress_to_string, evaluate_user, get_asset_prices_db, get_reserve_config_db,
        update_rate, user_positions_from_ledger_entry,
    },
    transaction_builder::BlendTxBuilder,
    types::{Action, Config, Event, UserPositions},
};
use async_trait::async_trait;
use ed25519_dalek::SigningKey;
use soroban_rpc::{Client, Event as SorobanEvent};
use std::{collections::HashMap, str::FromStr, thread::sleep, time::Duration, vec};
use stellar_xdr::curr::{
    AccountId, LedgerEntryData, LedgerKeyContractData, Limits, PublicKey, ReadXdr, ScAddress,
    ScMap, ScMapEntry, ScSpecTypeDef, ScSymbol, ScVal, ScVec, StringM, Uint256, VecM,
};
use tracing::{error, info};

pub struct BlendAuctioneer {
    /// Soroban RPC client for interacting with chain
    rpc: Client,
    /// The path to the database directory
    db_manager: DbManager,
    /// Assets in pools
    assets: Vec<String>,
    /// Vec of Blend pool addresses to create auctions for
    pools: Vec<String>,
    /// Map pool users and their positions
    /// - only stores users with health factor < 5
    /// - only stores users with relevant assets
    /// HashMap<PoolId, HashMap<UserId, UserPositions>>
    users: HashMap<String, HashMap<String, UserPositions>>,
    /// Our signing address
    us: SigningKey,
    /// Our public key
    pub us_public: String,
    // Backstop token address
    pub backstop_token_address: String,
    // Oracle id
    oracle_id: String,
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
            us_public: ScAddress::Account(AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(
                signing_key.verifying_key().to_bytes(),
            ))))
            .to_string(),
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
                match self.user_has_liquidation(pool.clone(), user.as_str()).await {
                    Ok(res) => {
                        if res {
                            continue;
                        }
                    }
                    Err(err) => error!(
                        "Failed to check {} for liquidation auction with error: {}",
                        user.clone(),
                        err
                    ),
                }
                match self.get_user_position(pool.clone(), user.as_str()).await {
                    Ok(_) => (),
                    Err(err) => error!(
                        "Failed to get positions for user {} with error: {}",
                        user.clone(),
                        err
                    ),
                }
            }
        }
        info!("synced auctioneer state");
        Ok(())
    }

    // Process incoming events
    async fn process_event(&mut self, event: Event) -> Vec<Action> {
        let mut retry_counter = 0;
        while retry_counter < 100 {
            match event {
                Event::SorobanEvents(ref soroban_event) => {
                    let event = *soroban_event.clone();
                    let result = self.process_soroban_events(event.clone()).await;
                    match result {
                        Ok(actions) => return actions,
                        Err(e) => {
                            retry_counter += 1;
                            info!("retrying soroban event processing");
                            if retry_counter == 100 {
                                let log = format!(
                                    "failed to process soroban event: {:#?} with error: {}\n",
                                    event.clone(),
                                    e
                                );
                                log_error(&log).unwrap();
                            }
                            sleep(Duration::from_millis(500));
                        }
                    }
                }
                Event::NewBlock(ref block) => {
                    let result = self.process_new_block_event(*block.clone()).await;
                    match result {
                        Ok(actions) => return actions,
                        Err(e) => {
                            retry_counter += 1;
                            info!("retrying new block event processing");
                            if retry_counter == 100 {
                                let log = format!(
                                    "failed to process new block event: {:#?} with error: {}\n",
                                    block.clone(),
                                    e
                                );
                                log_error(&log).unwrap();
                            }
                            sleep(Duration::from_millis(500));
                        }
                    }
                }
            }
        }
        return Vec::new();
    }
}

impl BlendAuctioneer {
    // Process new orders as they come in.
    async fn process_soroban_events(&mut self, event: SorobanEvent) -> Result<Vec<Action>> {
        let mut actions = Vec::new();
        //should build pending auctions and remove or modify pending auctions that are filled or partially filled by someone else
        let pool_id = event.contract_id;
        let mut name: String = Default::default();
        //Get contract function name from topics
        let topic = ScVal::from_xdr_base64(event.topic[0].as_bytes(), Limits::none())?;
        match topic {
            ScVal::Symbol(function_name) => {
                name = function_name.0.to_string();
            }
            _ => (),
        }
        let data = ScVal::from_xdr_base64(event.value.as_bytes(), Limits::none())?;
        //Deserialize event body cases
        match name.as_str() {
            "new_liquidation_auction" => {
                let user = decode_scaddress_to_string(&ScVal::from_xdr_base64(
                    event.topic[1].as_bytes(),
                    Limits::none(),
                )?);

                // remove user from users list since they are being liquidated
                self.users
                    .entry(pool_id.clone())
                    .or_default()
                    .remove(&user.to_string());
            }
            "delete_liquidation_auction" => {
                // If this was an auction we were planning on filling, remove it from the pending list
                let user = decode_scaddress_to_string(&ScVal::from_xdr_base64(
                    event.topic[1].as_bytes(),
                    Limits::none(),
                )?);

                // add user back to users
                match self
                    .get_user_position(pool_id.clone(), user.to_string().as_str())
                    .await
                {
                    Ok(_) => (),
                    Err(err) => error!(
                        "Failed to get positions for user {} with error: {}",
                        user.to_string(),
                        err
                    ),
                }
            }
            "fill_auction" => {
                let liquidated_id = decode_scaddress_to_string(&ScVal::from_xdr_base64(
                    event.topic[1].as_bytes(),
                    Limits::none(),
                )?);
                let mut auction_type = 0;
                match ScVal::from_xdr_base64(event.topic[2].as_bytes(), Limits::none())? {
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
                    match self
                        .get_user_position(
                            pool_id.clone(),
                            liquidated_id.clone().to_string().as_str(),
                        )
                        .await
                    {
                        Ok(score) => {
                            //check if a bad debt call is necessary
                            if score.is_some() && score.unwrap() != 1 {
                                let action = self.act_on_score(
                                    &liquidated_id.to_string().as_str(),
                                    &pool_id,
                                    score.unwrap(),
                                );
                                if action.is_some() {
                                    actions.push(action.unwrap());
                                }
                            }
                        }
                        Err(err) => error!(
                            "Failed to get positions for user {} with error: {}",
                            liquidated_id.clone().to_string(),
                            err
                        ),
                    }
                }
            }
            "bad_debt" => {
                let user = decode_scaddress_to_string(&ScVal::from_xdr_base64(
                    event.topic[1].as_bytes(),
                    Limits::none(),
                )?);

                // remove user from users list since their positions were removed
                self.users
                    .entry(pool_id.clone())
                    .or_default()
                    .remove(&user.to_string());
                let tx_builder = BlendTxBuilder {
                    contract_id: pool_id.clone(),
                    signing_key: self.us.clone(),
                };
                actions.push(Action::SubmitTx(SubmitStellarTx {
                    op: tx_builder.new_bad_debt_auction(),
                    gas_bid_info: None,
                    signing_key: self.us.clone(),
                    max_retries: 100,
                }));
            }
            "set_reserve" => {
                let mut asset_id: String = Default::default();
                match &data {
                    ScVal::Vec(vec) => {
                        if let Some(vec) = vec {
                            let address = vec.clone().get(0).unwrap().to_owned();
                            match address {
                                ScVal::Address(address) => {
                                    asset_id = address.to_string();
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
                    .await?;
            }
            "supply" => {
                let asset_id = decode_scaddress_to_string(&ScVal::from_xdr_base64(
                    event.topic[1].as_bytes(),
                    Limits::none(),
                )?);

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
                    return Ok(Vec::new());
                }
                // Update reserve estimated b rate by using request.amount/b_tokens_minted from the emitted event
                let new_rate = update_rate(supply_amount, b_tokens_minted);
                match new_rate {
                    Ok(rate) => {
                        self.db_manager
                            .update_reserve_config_rate(&pool_id, &asset_id, rate, true)?;
                    }
                    Err(_) => {
                        get_reserve_config_db(
                            &self.rpc,
                            &vec![pool_id],
                            &vec![asset_id],
                            &self.db_manager,
                        )
                        .await?
                    }
                }
            }
            "withdraw" => {
                let asset_id = decode_scaddress_to_string(&ScVal::from_xdr_base64(
                    event.topic[1].as_bytes(),
                    Limits::none(),
                )?);
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
                    return Ok(Vec::new());
                }
                // Update reserve estimated b rate by using tokens out/b tokens burned from the emitted event
                let new_rate = update_rate(withdraw_amount, b_tokens_burned);
                match new_rate {
                    Ok(rate) => {
                        self.db_manager
                            .update_reserve_config_rate(&pool_id, &asset_id, rate, true)?;
                    }
                    Err(_) => {
                        get_reserve_config_db(
                            &self.rpc,
                            &vec![pool_id],
                            &vec![asset_id],
                            &self.db_manager,
                        )
                        .await?
                    }
                }
            }
            "supply_collateral" => {
                let asset_id = decode_scaddress_to_string(&ScVal::from_xdr_base64(
                    event.topic[1].as_bytes(),
                    Limits::none(),
                )?);
                let user = decode_scaddress_to_string(&ScVal::from_xdr_base64(
                    event.topic[2].as_bytes(),
                    Limits::none(),
                )?);

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
                    return Ok(Vec::new());
                }
                self.update_user(
                    &pool_id,
                    &user.to_string().as_str(),
                    &asset_id,
                    b_tokens_minted,
                    true,
                )
                .await?;

                // Update reserve's estimated b rate by using request.amount/b_tokens_minted from the emitted event

                let new_rate = update_rate(supply_amount, b_tokens_minted);
                match new_rate {
                    Ok(rate) => {
                        self.db_manager
                            .update_reserve_config_rate(&pool_id, &asset_id, rate, true)?;
                    }
                    Err(_) => {
                        get_reserve_config_db(
                            &self.rpc,
                            &vec![pool_id],
                            &vec![asset_id],
                            &self.db_manager,
                        )
                        .await?
                    }
                }
            }
            "withdraw_collateral" => {
                let asset_id = decode_scaddress_to_string(&ScVal::from_xdr_base64(
                    event.topic[1].as_bytes(),
                    Limits::none(),
                )?);
                let user = decode_scaddress_to_string(&ScVal::from_xdr_base64(
                    event.topic[2].as_bytes(),
                    Limits::none(),
                )?);
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
                    return Ok(Vec::new());
                }
                // Update users collateral positions
                self.update_user(
                    &pool_id,
                    &user.to_string().as_str(),
                    &asset_id,
                    -b_tokens_burned,
                    true,
                )
                .await?;

                // Update reserve estimated b rate by using tokens out/b tokens burned from the emitted event

                let new_rate = update_rate(withdraw_amount, b_tokens_burned);
                match new_rate {
                    Ok(rate) => {
                        self.db_manager
                            .update_reserve_config_rate(&pool_id, &asset_id, rate, true)?;
                    }
                    Err(_) => {
                        get_reserve_config_db(
                            &self.rpc,
                            &vec![pool_id],
                            &vec![asset_id],
                            &self.db_manager,
                        )
                        .await?
                    }
                }
            }
            "borrow" => {
                let asset_id = decode_scaddress_to_string(&ScVal::from_xdr_base64(
                    event.topic[1].as_bytes(),
                    Limits::none(),
                )?);
                let user = decode_scaddress_to_string(&ScVal::from_xdr_base64(
                    event.topic[2].as_bytes(),
                    Limits::none(),
                )?);

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
                    return Ok(Vec::new());
                }

                // Update users liability positions
                self.update_user(
                    &pool_id,
                    &user.to_string().as_str(),
                    &asset_id,
                    d_token_burned,
                    false,
                )
                .await?;

                // Update reserve estimated b rate by using request.amount/d tokens minted from the emitted event
                let new_rate = update_rate(borrow_amount, d_token_burned);
                match new_rate {
                    Ok(rate) => {
                        self.db_manager
                            .update_reserve_config_rate(&pool_id, &asset_id, rate, true)?;
                    }
                    Err(_) => {
                        get_reserve_config_db(
                            &self.rpc,
                            &vec![pool_id],
                            &vec![asset_id],
                            &self.db_manager,
                        )
                        .await?
                    }
                }
            }
            "repay" => {
                let asset_id = decode_scaddress_to_string(&ScVal::from_xdr_base64(
                    event.topic[1].as_bytes(),
                    Limits::none(),
                )?);
                let user = decode_scaddress_to_string(&ScVal::from_xdr_base64(
                    event.topic[2].as_bytes(),
                    Limits::none(),
                )?);

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
                    return Ok(Vec::new());
                }
                // Update users liability positions
                self.update_user(
                    &pool_id,
                    &user.to_string().as_str(),
                    &asset_id,
                    -d_token_burned,
                    false,
                )
                .await?;
                // Update reserve estimated d rate by using request.amount/d tokens burnt from the emitted event

                let new_rate = update_rate(repay_amount, d_token_burned);
                match new_rate {
                    Ok(rate) => {
                        self.db_manager
                            .update_reserve_config_rate(&pool_id, &asset_id, rate, true)?;
                    }
                    Err(_) => {
                        get_reserve_config_db(
                            &self.rpc,
                            &vec![pool_id],
                            &vec![asset_id],
                            &self.db_manager,
                        )
                        .await?
                    }
                }
            }
            //if oracle has events they can be handled here
            _ => (),
        }
        if actions.len() > 0 {
            return Ok(actions.to_vec());
        }
        Ok(Vec::new())
    }

    /// Process new block events, updating the internal state.
    async fn process_new_block_event(&mut self, event: NewBlock) -> Result<Vec<Action>> {
        let mut actions = Vec::new();
        //TEMP: check if liquidations are possible every 100 blocks since we're not getting oracle update events atm
        if event.number % 100 == 0 {
            info!("on block: {} ", event.number);
        }
        if event.number % 10 == 0 {
            get_asset_prices_db(
                &self.rpc,
                &self.oracle_id,
                &self.oracle_decimals,
                &self.assets,
                &self.db_manager,
            )
            .await?;
            for pool in self.pools.iter() {
                for users in self.users.get(pool).iter_mut() {
                    for user in users.iter() {
                        match evaluate_user(pool, user.1, &self.db_manager) {
                            Ok(score) => {
                                let action = self.act_on_score(&user.0, &pool, score);
                                if action.is_some() {
                                    info!("Creating liquidation auction for user: {}", user.0);
                                    actions.push(action.unwrap());
                                }
                            }
                            Err(err) => {
                                error!("Failed to evaluate user: {} with error: {}", user.0, err)
                            }
                        };
                        // create liquidation auction if needed
                    }
                }
            }
        }

        return Ok(actions);
    }

    async fn get_user_position(&mut self, pool_id: String, user_id: &str) -> Result<Option<u64>> {
        let reserve_data_key = ScVal::Vec(Some(ScVec::try_from(vec![
            ScVal::Symbol(ScSymbol::from(ScSymbol::from(StringM::from_str(
                "Positions",
            )?))),
            ScVal::Address(ScAddress::from_str(user_id)?),
        ])?));
        let position_ledger_key =
            stellar_xdr::curr::LedgerKey::ContractData(LedgerKeyContractData {
                contract: ScAddress::from_str(&pool_id)?,
                key: reserve_data_key,
                durability: stellar_xdr::curr::ContractDataDurability::Persistent,
            });
        let result = self
            .rpc
            .get_ledger_entries(&vec![position_ledger_key])
            .await?;
        if let Some(entries) = result.entries {
            for entry in entries {
                let value: LedgerEntryData =
                    LedgerEntryData::from_xdr_base64(entry.xdr, Limits::none())?;

                match &value {
                    LedgerEntryData::ContractData(data) => {
                        let user_id = match &data.key {
                            ScVal::Vec(vec) => {
                                if let Some(vec) = vec {
                                    decode_scaddress_to_string(&vec[1])
                                } else {
                                    return Ok(None);
                                }
                            }
                            _ => return Ok(None),
                        };
                        let user_position =
                            user_positions_from_ledger_entry(&value, &pool_id, &self.db_manager)?;

                        let score =
                            evaluate_user(&pool_id, &user_position, &self.db_manager).unwrap();
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
    async fn user_has_liquidation(&mut self, pool: String, user: &str) -> Result<bool> {
        let pool_id = ScAddress::from_str(&pool)?;
        let reserve_data_key = ScVal::Vec(Some(ScVec::try_from(vec![
            ScVal::Symbol(ScSymbol::from(ScSymbol::from(StringM::from_str(
                "Auction",
            )?))),
            ScVal::Map(Some(ScMap(VecM::try_from(vec![
                ScMapEntry {
                    key: from_string_primitive("auct_type", &ScSpecTypeDef::Symbol)?,
                    val: from_string_primitive("0", &ScSpecTypeDef::U32)?,
                },
                ScMapEntry {
                    key: from_string_primitive("user", &ScSpecTypeDef::Symbol)?,
                    val: ScVal::Address(ScAddress::from_str(user)?),
                },
            ])?))),
        ])?));
        let position_ledger_key =
            stellar_xdr::curr::LedgerKey::ContractData(LedgerKeyContractData {
                contract: pool_id.clone(),
                key: reserve_data_key,
                durability: stellar_xdr::curr::ContractDataDurability::Temporary,
            });
        let result = self
            .rpc
            .get_ledger_entries(&vec![position_ledger_key])
            .await?;
        if let Some(entries) = result.entries {
            for entry in entries {
                let value: LedgerEntryData =
                    LedgerEntryData::from_xdr_base64(entry.xdr, Limits::none())?;

                match &value {
                    LedgerEntryData::ContractData(_) => {
                        info!(
                            "Found outstanding user liquidation auction for: {:?}",
                            user.to_string()
                        );
                        return Ok(true);
                    }
                    _ => (),
                }
            }
        }

        Ok(false)
    }

    fn act_on_score(&self, user: &str, pool: &String, score: u64) -> Option<Action> {
        let tx_builder = BlendTxBuilder {
            contract_id: pool.clone(),
            signing_key: self.us.clone(),
        };
        if score == 0 {
            // Code to execute if the value is None
            return Some(Action::SubmitTx(SubmitStellarTx {
                op: tx_builder.bad_debt(user),
                gas_bid_info: None,
                signing_key: self.us.clone(),
                max_retries: 100,
            }));
        }

        if score > 2 {
            // Code to execute if the value is None
            return Some(Action::SubmitTx(SubmitStellarTx {
                op: tx_builder.new_liquidation_auction(user, score),
                gas_bid_info: None,
                signing_key: self.us.clone(),
                max_retries: 100,
            }));
        }
        None
    }

    // Updates user positions based on action
    // - If we do not have the user tracked we add them
    // - If we do have them tracked and they are adding an unsupported asset or their score is 1 we remove them
    async fn update_user(
        &mut self,
        pool_id: &String,
        user_id: &str,
        asset_id: &String,
        amount: i128,
        collateral: bool,
    ) -> Result<()> {
        self.db_manager.add_user(&user_id.to_string()).unwrap();
        let pool = self.users.entry(pool_id.clone()).or_default();
        if let Some(positions) = pool.get_mut(&user_id.to_string()) {
            if collateral {
                let balance = positions.collateral.entry(asset_id.clone()).or_insert(0);
                *balance += amount;
            } else {
                let balance = positions.liabilities.entry(asset_id.clone()).or_insert(0);
                *balance += amount;
            }

            // user's borrowing power is going up so we should potentially drop them
            if (collateral && amount > 0) || (!collateral && amount < 0) {
                let score = evaluate_user(&pool_id, &positions, &self.db_manager).unwrap();
                if score == 1 {
                    pool.remove(&user_id.to_string());
                }
            }
        } else if (collateral && amount < 0) || (!collateral && amount > 0) {
            // User's borrowing power is going down so we should potentially add them
            self.get_user_position(pool_id.clone(), user_id).await?;
        }
        Ok(())
    }
}
