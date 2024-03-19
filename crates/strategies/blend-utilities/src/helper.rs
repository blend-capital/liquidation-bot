use std::{collections::HashMap, str::FromStr};

use crate::{
    transaction_builder::BlendTxBuilder,
    types::{AuctionData, ReserveConfig, UserPositions},
};
use anyhow::Result;
use ed25519_dalek::SigningKey;
use rusqlite::{params, Connection};
use soroban_cli::rpc::Client;
use soroban_spec_tools::from_string_primitive;
use stellar_xdr::curr::{
    Hash, InvokeContractArgs, InvokeHostFunctionOp, LedgerEntryData, LedgerKey,
    LedgerKeyContractData, Limits, Memo, MuxedAccount, Operation, Preconditions, ReadXdr,
    ScAddress, ScSpecTypeDef, ScSymbol, ScVal, ScVec, StringM, Transaction, TransactionEnvelope,
    TransactionV1Envelope, Uint256, VecM,
};

pub fn decode_entry_key(key: &ScVal) -> String {
    match key {
        ScVal::String(string) => {
            return string.to_string();
        }
        ScVal::Symbol(symbol) => symbol.to_string(),
        ScVal::Vec(vec) => {
            if let Some(vec) = vec {
                match &vec[0] {
                    ScVal::Symbol(string) => {
                        return string.to_string();
                    }
                    _ => {
                        return "".to_string();
                    }
                }
            } else {
                return "".to_string();
            }
        }
        _ => {
            return "".to_string();
        }
    }
}
pub fn decode_to_asset_amount_map(map: &ScVal) -> HashMap<Hash, i128> {
    let mut asset_amount_map: HashMap<Hash, i128> = HashMap::new();
    match map {
        ScVal::Map(asset_map) => {
            for asset in asset_map.clone().unwrap().iter() {
                let mut asset_address: Hash = Hash([0; 32]);
                match asset.key.clone() {
                    ScVal::Address(address) => match address {
                        ScAddress::Account(account) => match account.0 {
                            stellar_xdr::curr::PublicKey::PublicKeyTypeEd25519(pub_key) => {
                                asset_address = Hash(pub_key.0);
                            }
                        },
                        ScAddress::Contract(contract) => {
                            asset_address = contract;
                        }
                    },
                    _ => (),
                }
                let amount: i128 = match &asset.val {
                    ScVal::I128(amount) => amount.into(),
                    _ => 0,
                };
                asset_amount_map.insert(asset_address, amount);
            }
        }
        _ => (),
    }
    return asset_amount_map;
}

pub fn decode_scaddress_to_hash(address: &ScVal) -> Hash {
    match address {
        ScVal::Address(address) => match address {
            ScAddress::Account(account_id) => match &account_id.0 {
                stellar_xdr::curr::PublicKey::PublicKeyTypeEd25519(key) => {
                    return Hash(key.0);
                }
            },
            ScAddress::Contract(contract_id) => {
                return contract_id.to_owned();
            }
        },
        _ => {
            //TODO decide if this should return an Error
            return Hash([0; 32]);
        }
    }
}

pub fn decode_i128_to_native(scval: &ScVal) -> i128 {
    match scval {
        ScVal::I128(num) => {
            return num.into();
        }
        _ => 0,
    }
}

pub fn decode_auction_data(auction_data: ScVal) -> AuctionData {
    let mut bid: HashMap<Hash, i128> = HashMap::new(); //TODO grab from event
    let mut lot: HashMap<Hash, i128> = HashMap::new(); //TODO grab from event
    let mut block = 0;
    match auction_data {
        ScVal::Map(map) => {
            if let Some(map) = map {
                for entry in map.iter() {
                    let key = decode_entry_key(&entry.key);
                    match key.as_str() {
                        "bid" => {
                            bid = decode_to_asset_amount_map(&entry.val);
                        }
                        "lot" => {
                            lot = decode_to_asset_amount_map(&entry.val);
                        }
                        //TODO decide whether we need this
                        "block" => match &entry.val {
                            ScVal::U32(num) => {
                                block = num.to_owned();
                            }
                            _ => (),
                        },
                        _ => (),
                    }
                }
            }
        }
        _ => (),
    }
    return AuctionData { bid, lot, block };
}
//Returns (index, collateral_factor, liability_factor,scalar)
pub fn reserve_config_from_ledger_entry(
    ledger_entry_data: &LedgerEntryData,
) -> (u32, u32, u32, i128) {
    let mut collateral_factor: u32 = 0;
    let mut liability_factor: u32 = 0;
    let mut index: u32 = 0;
    let mut decimals: u32 = 0;
    match ledger_entry_data {
        LedgerEntryData::ContractData(data) => match &data.val {
            ScVal::Map(map) => {
                if let Some(data_entry_map) = map {
                    for entry in data_entry_map.iter() {
                        let key = decode_entry_key(&entry.key);
                        match key.as_str() {
                            "c_factor" => match &entry.val {
                                ScVal::U32(num) => {
                                    collateral_factor = *num;
                                }
                                _ => (),
                            },
                            "l_factor" => match &entry.val {
                                ScVal::U32(num) => {
                                    liability_factor = *num;
                                }
                                _ => (),
                            },
                            "index" => match &entry.val {
                                ScVal::U32(num) => {
                                    index = *num;
                                }
                                _ => (),
                            },
                            "decimals" => match &entry.val {
                                ScVal::U32(num) => {
                                    decimals = *num;
                                }
                                _ => (),
                            },
                            _ => (),
                        }
                    }
                }
            }
            _ => (),
        },
        _ => println!("Error: expected LedgerEntryData to be ContractData"),
    }
    let scalar = 10i128.pow(decimals);
    println!("index {}", index);
    println!("cfactor {}", collateral_factor);
    println!("scalar {}", scalar);
    return (index, collateral_factor, liability_factor, scalar);
}

pub fn reserve_data_from_ledger_entry(ledger_entry_data: &LedgerEntryData) -> (i128, i128) {
    let mut b_rate: i128 = 0;
    let mut d_rate: i128 = 0;

    match ledger_entry_data {
        LedgerEntryData::ContractData(data) => match &data.val {
            ScVal::Map(map) => {
                if let Some(data_entry_map) = map {
                    for entry in data_entry_map.iter() {
                        let key = decode_entry_key(&entry.key);
                        match key.as_str() {
                            "b_rate" => {
                                b_rate = decode_i128_to_native(&entry.val);
                            }
                            "d_rate" => {
                                d_rate = decode_i128_to_native(&entry.val);
                            }
                            _ => (),
                        }
                    }
                }
            }
            _ => (),
        },
        _ => println!("Error: expected LedgerEntryData to be ContractData"),
    }
    return (b_rate, d_rate);
}
pub fn user_positions_from_ledger_entry(
    ledger_entry_data: &LedgerEntryData,
    pool: &Hash,
) -> Result<UserPositions> {
    let mut user_positions = UserPositions {
        collateral: HashMap::default(),
        liabilities: HashMap::default(),
    };
    let db = Connection::open("blend_assets.db").unwrap();
    match ledger_entry_data {
        LedgerEntryData::ContractData(data) => match &data.val {
            ScVal::Map(map) => {
                if let Some(data_entry_map) = map {
                    for entry in data_entry_map.iter() {
                        let key = decode_entry_key(&entry.key);
                        match key.as_str() {
                            "liabilities" => match &entry.val {
                                ScVal::Map(map) => {
                                    if let Some(map) = map {
                                        for entry in map.0.iter() {
                                            match entry.key {
                                                ScVal::U32(index) => {
                                                    user_positions.liabilities.insert(
                                                        ReserveConfig::from_db_w_index(
                                                            pool, &index, &db,
                                                        )?
                                                        .asset,
                                                        decode_i128_to_native(&entry.val),
                                                    );
                                                }
                                                _ => (),
                                            }
                                        }
                                    }
                                }
                                _ => (),
                            },
                            "collateral" => match &entry.val {
                                ScVal::Map(map) => {
                                    if let Some(map) = map {
                                        for entry in map.0.iter() {
                                            match entry.key {
                                                ScVal::U32(index) => {
                                                    println!("index {}", index);
                                                    println!(
                                                        "pool {}",
                                                        ScAddress::Contract(pool.clone())
                                                            .to_string()
                                                    );
                                                    user_positions.collateral.insert(
                                                        ReserveConfig::from_db_w_index(
                                                            pool, &index, &db,
                                                        )?
                                                        .asset,
                                                        decode_i128_to_native(&entry.val),
                                                    );
                                                }
                                                _ => (),
                                            }
                                        }
                                    }
                                }
                                _ => (),
                            },
                            _ => (),
                        }
                    }
                }
            }
            _ => (),
        },
        _ => println!("Error: expected LedgerEntryData to be ContractData"),
    }
    db.close().unwrap();
    Ok(user_positions)
}

// computes the value of reserve assets both before and after collateral or liability factors are applied
pub fn sum_adj_asset_values(
    assets: HashMap<Hash, i128>,
    pool: &Hash,
    collateral: bool,
) -> Result<(i128, i128)> {
    let db = Connection::open("blend_assets.db").unwrap();
    let mut value: i128 = 0;
    let mut adjusted_value: i128 = 0;
    for (asset, amount) in assets.iter() {
        let price = db
            .query_row(
                "SELECT price FROM asset_prices WHERE address = ?",
                [ScAddress::Contract(asset.clone()).to_string()],
                |row| row.get::<_, isize>(0),
            )
            .unwrap() as i128;
        let config = ReserveConfig::from_db_w_asset(pool, asset, &db).unwrap();

        let modifiers: (i128, i128) = if collateral {
            println!("asset :{}", ScAddress::Contract(asset.clone()).to_string());
            println!("c price {} ", price);

            println!("b rate {}", config.est_b_rate);
            (config.est_b_rate, config.collateral_factor as i128)
        } else {
            println!("l price {} ", price);
            println!("d rate {}", config.est_d_rate);
            let test = price * amount / config.scalar;
            println!("is ok? {}", test);
            (
                config.est_d_rate,
                1e14 as i128 / config.liability_factor as i128,
            )
        };
        let raw_val = price * amount / config.scalar * modifiers.0 / 1e9 as i128;
        let adj_val = raw_val * modifiers.1 / 1e7 as i128;

        value += raw_val;
        adjusted_value += adj_val;
    }
    println!("value {}", value);
    db.close().unwrap();
    Ok((value, adjusted_value))
}

// returns 0 if user should be ignored, 1 if user should be watched, a pct if user should be liquidated for the given pct
pub fn evaluate_user(pool: &Hash, user_positions: &UserPositions) -> Result<u64> {
    let (collateral_value, adj_collateral_value) =
        sum_adj_asset_values(user_positions.collateral.clone(), pool, true)?;
    let (liabilities_value, adj_liabilities_value) =
        sum_adj_asset_values(user_positions.liabilities.clone(), pool, false)?;
    let remaining_power = adj_collateral_value - adj_liabilities_value;
    println!("adj collateral {}", adj_collateral_value);
    println!("adj liabilities {}", adj_liabilities_value);
    if adj_collateral_value == 0 && adj_liabilities_value > 0 {
        Ok(0) //we need to do a bad debt on these guys
    } else if remaining_power > adj_liabilities_value * 5 || adj_collateral_value == 0 {
        // user's HF is over 5 so we ignore them// TODO: this might not be large enough
        // we also ignore user's with no collateral
        Ok(1)
    } else if remaining_power > 0 {
        Ok(2) // User's cooling but we still wanna track
    } else {
        const SCL_7: i128 = 1e7 as i128;
        let inv_lf = adj_liabilities_value * SCL_7 / liabilities_value;
        let cf = adj_collateral_value * SCL_7 / collateral_value;
        let numerator = adj_liabilities_value * 1_100_0000 / SCL_7 - adj_collateral_value;
        let est_incentive = SCL_7 + (SCL_7 - cf * SCL_7 / inv_lf) / 2;
        let denominator = inv_lf * 1_100_0000 / SCL_7 - cf * est_incentive / SCL_7;
        let mut pct = 0;
        if denominator != 0 && liabilities_value != 0 {
            pct = (numerator * SCL_7 / denominator * 100 / liabilities_value) as u64;
        }
        println!("pct {}", pct);
        Ok(pct.clamp(1, 100))
    }
}

pub async fn bstop_token_to_usdc(
    rpc: &Client,
    bstop_tkn_address: Hash,
    backstop: Hash,
    lp_amount: i128,
    usdc_address: Hash,
) -> Result<i128, ()> {
    println!("");
    println!("getting bstop token value");
    // A random key is fine for simulation
    let key = SigningKey::from_bytes(&[0; 32]);

    // fn wdr_tokn_amt_in_get_lp_tokns_out(
    //     e: Env,
    //     token_out: Address,
    //     pool_amount_in: i128,
    //     min_amount_out: i128,
    //     user: Address,
    // ) -> i128;
    println!("lp amount {}", lp_amount);
    let op = Operation {
        source_account: None,
        body: stellar_xdr::curr::OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
            host_function: stellar_xdr::curr::HostFunction::InvokeContract(InvokeContractArgs {
                contract_address: ScAddress::Contract(bstop_tkn_address),
                function_name: ScSymbol::try_from("wdr_tokn_amt_in_get_lp_tokns_out").unwrap(),
                args: VecM::try_from(vec![
                    ScVal::Address(ScAddress::Contract(usdc_address)),
                    from_string_primitive(lp_amount.to_string().as_str(), &ScSpecTypeDef::I128)
                        .unwrap(),
                    from_string_primitive("0".to_string().as_str(), &ScSpecTypeDef::I128).unwrap(),
                    ScVal::Address(ScAddress::Contract(backstop)),
                ])
                .unwrap(),
            }),
            auth: VecM::default(),
        }),
    };
    let transaction: TransactionEnvelope = TransactionEnvelope::Tx(TransactionV1Envelope {
        tx: Transaction {
            source_account: stellar_xdr::curr::MuxedAccount::Ed25519(Uint256(
                key.verifying_key().to_bytes(),
            )),
            fee: 10000,
            seq_num: stellar_xdr::curr::SequenceNumber(10),
            cond: stellar_xdr::curr::Preconditions::None,
            memo: Memo::None,
            operations: vec![op].try_into()?,
            ext: stellar_xdr::curr::TransactionExt::V0,
        },
        signatures: VecM::default(),
    });
    let sim_result = rpc.simulate_transaction(&transaction).await.unwrap();
    let contract_function_result =
        ScVal::from_xdr_base64(sim_result.results[0].xdr.clone(), Limits::none()).unwrap();
    let usdc_out: Option<i128> = match &contract_function_result {
        ScVal::I128(value) => Some(value.into()),
        _ => None,
    };
    return Ok(usdc_out.unwrap());
}

pub fn update_rate(
    pool_id: Hash,
    asset_id: Hash,
    numerator: i128,
    denominator: i128,
    b_rate: bool,
) -> Result<(), (Connection, rusqlite::Error)> {
    let db = Connection::open("blend_assets.db").unwrap();

    let rate = numerator * 1e9 as i128 / denominator;
    assert!(rate.gt(&1_000_0000));
    let key = (ScAddress::Contract(asset_id.clone()).to_string()
        + &ScAddress::Contract(pool_id.clone()).to_string())
        .to_string();
    if b_rate {
        db.execute(
            "UPDATE pool_asset_data SET bRate = ?1 WHERE key = ?2",
            params![rate as u64, key,],
        )
        .unwrap()
    } else {
        db.execute(
            "UPDATE pool_asset_data SET dRate = ?1 WHERE key = ?2",
            params![rate as u64, key,],
        )
        .unwrap()
    };
    db.close()
}

pub fn pool_has_asset(pool: &Hash, asset: &String, db: &Connection) -> bool {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pool_asset_data WHERE key = ?1",
        [ScAddress::Contract(pool.clone()).to_string() + &asset],
        |row| row.get(0),
    )
    .unwrap()
}

pub fn populate_db(db: &Connection, assets: &Vec<Hash>) -> Result<(), rusqlite::Error> {
    println!("creating asset_prices table");
    db.execute(
        "CREATE table if not exists asset_prices (
            address string primary key,
            price integer not null
         )",
        [],
    )?;
    println!("creating pool_asset_data table");
    //TODO: this setup will fail, need a better key (pool_address, asset_address)
    db.execute(
        "create table if not exists pool_asset_data (
            key string primary key,
            pool_address string not null,
            address string not null,
            asset_index integer not null,
            dRate integer not null,
            bRate integer not null,
            collateralFactor integer not null,
            liabilityFactor integer not null,
            scalar integer not null
         )",
        [],
    )?;

    println!("populating tables");
    let placeholder_int = 1i64;
    for asset in assets.clone() {
        let asset_str = ScAddress::Contract(asset).to_string();
        println!("asset_str: {:?}", asset_str);
        let result = db.execute(
            "INSERT INTO asset_prices (address, price) VALUES (?, ?)",
            params![asset_str, placeholder_int],
        );
        match result {
            Ok(_) => println!("Insert successful"),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                println!("Asset already exists in table");
            }
            Err(err) => println!("Other error: {}", err),
        }
        println!("added asset price");
    }
    Ok(())
}

pub async fn get_asset_prices_db(
    rpc: &Client,
    oracle_id: &Hash,
    oracle_decimals: &u32,
    assets: &Vec<Hash>,
) -> Result<()> {
    let db = Connection::open("blend_assets.db")?;
    // A random key is fine for simulation
    let key = SigningKey::from_bytes(&[0; 32]);
    // get asset prices from oracle
    for asset in assets.iter() {
        let tx_builder = BlendTxBuilder {
            contract_id: oracle_id.clone(),
            signing_key: key.clone(), //TODO: this should work fine without a real key
        };
        let op = tx_builder.get_last_price(asset).unwrap();
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
        let sim_result = rpc.simulate_transaction(&transaction).await?;
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
        // adjust price to seven decimals
        price = price * 10000000 / (10 as i128).pow(*oracle_decimals);
        db.execute(
            "UPDATE asset_prices SET price = ?2 WHERE address = ?1",
            [
                ScAddress::Contract(asset.clone()).to_string(),
                price.to_string(),
            ],
        )?;
    }
    db.close().unwrap();
    Ok(())
}

pub async fn get_reserve_config_db(
    rpc: &Client,
    pools: &Vec<Hash>,
    assets: &Vec<Hash>,
) -> Result<()> {
    let mut reserve_configs: HashMap<Hash, HashMap<Hash, ReserveConfig>> = HashMap::new();
    for pool in pools {
        let mut ledger_keys: Vec<LedgerKey> = Vec::new();
        for asset in assets {
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

        let result = rpc.get_ledger_entries(&ledger_keys).await.unwrap();
        if let Some(entries) = result.entries {
            for entry in entries {
                let value = LedgerEntryData::from_xdr_base64(entry.xdr, Limits::none()).unwrap();
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

                        let res_config = reserve_configs
                            .entry(pool.clone())
                            .or_default()
                            .entry(asset_id.clone())
                            .or_insert(ReserveConfig::default(asset_id.clone()));
                        match key.as_str() {
                            "ResData" => {
                                let (b_rate, d_rate) = reserve_data_from_ledger_entry(&value);
                                println!("b_rate {}", b_rate);
                                println!("d_rate {}", d_rate);
                                res_config.est_b_rate = b_rate;
                                res_config.est_d_rate = d_rate;
                            }
                            "ResConfig" => {
                                let (index, collateral_factor, liability_factor, scalar) =
                                    reserve_config_from_ledger_entry(&value);
                                res_config.index = index;
                                res_config.collateral_factor = collateral_factor;
                                res_config.liability_factor = liability_factor;
                                res_config.scalar = scalar;
                            }
                            _ => println!("Error: found unexpected key {}", key),
                        }
                    }
                    _ => (),
                }
            }
        }
    }
    let db = Connection::open("blend_assets.db")?;
    for pool in pools {
        for asset in assets {
            let res_config = match reserve_configs.get(pool).unwrap().get(asset) {
                Some(config) => config,
                None => {
                    continue;
                }
            };

            let pool_address_str = ScAddress::Contract(pool.clone()).to_string();
            let asset_address_str = ScAddress::Contract(asset.clone()).to_string();
            let db_key = (asset_address_str.clone() + &pool_address_str.clone()).to_string();
            db.execute(
                "INSERT OR REPLACE INTO pool_asset_data (key, bRate, dRate, asset_index, collateralFactor, liabilityFactor, scalar, pool_address, address) VALUES (?7, ?1, ?2, ?3, ?4, ?5, ?6, ?8, ?9)",
                params![
                    res_config.est_b_rate as u64,
                    res_config.est_d_rate as u64,
                    res_config.index,
                    res_config.collateral_factor,
                    res_config.liability_factor,
                    res_config.scalar as u64,
                    db_key,
                    pool_address_str,
                    asset_address_str,
                ],
            )?;
        }
    }

    db.close().unwrap();
    Ok(())
}
