use core::panic;
use std::{collections::HashMap, str::FromStr};

use crate::{
    constants::{SCALAR_7, SCALAR_9},
    db_manager::DbManager,
    transaction_builder::BlendTxBuilder,
    types::{AuctionData, ReserveConfig, UserPositions},
};
use anyhow::{Error, Result};
use ed25519_dalek::SigningKey;
use soroban_fixed_point_math::FixedPoint;
use stellar_rpc_client::Client;
use soroban_spec_tools::from_string_primitive;
use stellar_xdr::curr::{
    InvokeContractArgs, InvokeHostFunctionOp, LedgerEntryData, LedgerKey, LedgerKeyContractData,
    Limits, Memo, MuxedAccount, Operation, Preconditions, ReadXdr, ScAddress, ScSpecTypeDef,
    ScSymbol, ScVal, ScVec, StringM, Transaction, TransactionEnvelope, TransactionV1Envelope,
    Uint256, VecM,
};
use tracing::error;

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
pub fn decode_to_asset_amount_map(map: &ScVal) -> HashMap<String, i128> {
    let mut asset_amount_map: HashMap<String, i128> = HashMap::new();
    match map {
        ScVal::Map(asset_map) => {
            for asset in asset_map.clone().unwrap().iter() {
                let mut asset_address: String = Default::default();
                match asset.key.clone() {
                    ScVal::Address(address) => asset_address = address.to_string(),
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

pub fn decode_scaddress_to_string(address: &ScVal) -> String {
    match address {
        ScVal::Address(address) => address.to_string(),
        _ => {
            panic!("Error: expected ScVal to be Address");
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
    let mut bid: HashMap<String, i128> = HashMap::new();
    let mut lot: HashMap<String, i128> = HashMap::new();
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
        _ => panic!("Error: expected LedgerEntryData to be ContractData"),
    }
    let scalar = 10i128.pow(decimals);

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
        _ => panic!("Error: expected LedgerEntryData to be ContractData"),
    }
    return (b_rate, d_rate);
}
pub fn user_positions_from_ledger_entry(
    ledger_entry_data: &LedgerEntryData,
    pool: &String,
    db_manager: &DbManager,
) -> Result<UserPositions> {
    let mut user_positions = UserPositions {
        collateral: HashMap::default(),
        liabilities: HashMap::default(),
    };
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
                                                        db_manager
                                                            .get_reserve_config_from_index(
                                                                pool, &index,
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
                                                    user_positions.collateral.insert(
                                                        db_manager
                                                            .get_reserve_config_from_index(
                                                                pool, &index,
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
        _ => panic!("Error: expected LedgerEntryData to be ContractData"),
    }
    Ok(user_positions)
}

// computes the value of reserve assets both before and after collateral or liability factors are applied
pub fn sum_adj_asset_values(
    assets: HashMap<String, i128>,
    pool: &String,
    collateral: bool,
    db_manager: &DbManager,
) -> Result<(i128, i128)> {
    let mut value: i128 = 0;
    let mut adjusted_value: i128 = 0;
    for (asset, amount) in assets.iter() {
        let price = db_manager.get_asset_price(&asset)?;
        let config = db_manager.get_reserve_config_from_asset(pool, asset)?;
        let (raw_val, adj_val) = calc_position_value(config, price, *amount, collateral);
        value += raw_val;
        adjusted_value += adj_val;
    }
    Ok((value, adjusted_value))
}

// Returns the raw and adjusted value of a user's position (raw,adjusted)
fn calc_position_value(
    config: ReserveConfig,
    price: i128,
    amount: i128,
    collateral: bool,
) -> (i128, i128) {
    let modifiers: (i128, i128) = if collateral {
        (config.est_b_rate, config.collateral_factor as i128)
    } else {
        (
            config.est_d_rate,
            1e14 as i128 / config.liability_factor as i128,
        )
    };
    let raw_val = price
        .fixed_mul_floor(amount, config.scalar)
        .unwrap()
        .fixed_mul_floor(modifiers.0, SCALAR_9)
        .unwrap();
    let adj_val = raw_val.fixed_mul_floor(modifiers.1, SCALAR_7).unwrap();

    (raw_val, adj_val)
}

// returns 0 if user should be ignored, 1 if user should be watched, a pct if user should be liquidated for the given pct
pub fn evaluate_user(
    pool: &String,
    user_positions: &UserPositions,
    db_manager: &DbManager,
) -> Result<u64> {
    let (collateral_value, adj_collateral_value) =
        sum_adj_asset_values(user_positions.collateral.clone(), pool, true, db_manager)?;
    let (liabilities_value, adj_liabilities_value) =
        sum_adj_asset_values(user_positions.liabilities.clone(), pool, false, db_manager)?;
    let remaining_power = adj_collateral_value - adj_liabilities_value;

    if adj_collateral_value == 0 && adj_liabilities_value > 0 {
        Ok(0) //we need to do a bad debt on these guys
    } else if remaining_power > adj_liabilities_value * 5 || adj_collateral_value == 0 {
        // user's HF is over 5 so we ignore them
        // we also ignore user's with no collateral
        Ok(1)
    } else if remaining_power > 0 {
        Ok(2) // User's cooling but we still wanna track
    } else {
        // we need to liquidate this user - calculate the percent to liquidate for
        Ok(get_liq_percent(
            adj_liabilities_value,
            liabilities_value,
            adj_collateral_value,
            collateral_value,
        ))
    }
}
fn get_liq_percent(
    adj_liabilities_value: i128,
    liabilities_value: i128,
    adj_collateral_value: i128,
    collateral_value: i128,
) -> u64 {
    let inv_lf = adj_liabilities_value
        .fixed_div_floor(liabilities_value, SCALAR_7)
        .unwrap();
    let cf = adj_collateral_value
        .fixed_div_floor(collateral_value, SCALAR_7)
        .unwrap();
    let numerator = adj_liabilities_value
        .fixed_mul_floor(1_100_0000, SCALAR_7)
        .unwrap()
        - adj_collateral_value;
    let est_incentive = SCALAR_7 + (SCALAR_7 - cf.fixed_div_floor(inv_lf, SCALAR_7).unwrap()) / 2;
    let denominator = inv_lf.fixed_mul_floor(1_100_0000, SCALAR_7).unwrap()
        - cf.fixed_mul_floor(est_incentive, SCALAR_7).unwrap();
    let pct = numerator
        .fixed_div_floor(denominator, SCALAR_7)
        .unwrap_or(0)
        .fixed_div_floor(liabilities_value, 100)
        .unwrap_or(0) as u64;
    pct.clamp(1, 100)
}

pub async fn bstop_token_to_usdc(
    rpc: &Client,
    bstop_tkn_address: String,
    backstop: String,
    lp_amount: i128,
    usdc_address: String,
) -> Result<i128> {
    // A random key is fine for simulation
    let key = SigningKey::from_bytes(&[0; 32]);
    let op = Operation {
        source_account: None,
        body: stellar_xdr::curr::OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
            host_function: stellar_xdr::curr::HostFunction::InvokeContract(InvokeContractArgs {
                contract_address: ScAddress::from_str(&bstop_tkn_address)?,
                function_name: ScSymbol::try_from("wdr_tokn_amt_in_get_lp_tokns_out").unwrap(),
                args: VecM::try_from(vec![
                    ScVal::Address(ScAddress::from_str(&usdc_address)?),
                    from_string_primitive(lp_amount.to_string().as_str(), &ScSpecTypeDef::I128)?,
                    from_string_primitive("0".to_string().as_str(), &ScSpecTypeDef::I128)?,
                    ScVal::Address(ScAddress::from_str(&backstop)?),
                ])?,
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
    let sim_result = rpc.simulate_transaction(&transaction).await;
    let usdc_out = match sim_result {
        Ok(sim_result) => {
            let contract_function_result =
                ScVal::from_xdr_base64(sim_result.results[0].xdr.clone(), Limits::none())?;
            match &contract_function_result {
                ScVal::I128(value) => Some(value.into()),
                _ => None,
            }
        }
        Err(_) => {
            error!("Error: failed to simulate backstop token USDC withdrawal - using balance method instead");
            let total_comet_usdc =
                get_balance(rpc, bstop_tkn_address.clone(), usdc_address.clone()).await?;
            let total_comet_tokens = total_comet_tokens(rpc, bstop_tkn_address.clone()).await?;
            Some(
                total_comet_usdc
                    .fixed_div_floor(total_comet_tokens, SCALAR_7)
                    .unwrap()
                    .fixed_mul_floor(lp_amount, SCALAR_7)
                    .unwrap(),
            )
        }
    };
    return Ok(usdc_out.unwrap());
}

// Gets balance of an asset
pub async fn get_balance(rpc: &Client, user: String, asset: String) -> Result<i128> {
    // A random key is fine for simulation
    let key = SigningKey::from_bytes(&[0; 32]);
    let op = BlendTxBuilder {
        contract_id: asset.clone(),
        signing_key: key.clone(),
    }
    .get_balance(&user.clone().as_str());
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
        ScVal::from_xdr_base64(sim_result.results[0].xdr.clone(), Limits::none())?;
    let mut balance: i128 = 0;
    match &contract_function_result {
        ScVal::I128(value) => balance = value.into(),
        _ => (),
    }

    Ok(balance)
}

// Gets total comet tokens
pub async fn total_comet_tokens(rpc: &Client, bstop_tkn_address: String) -> Result<i128> {
    // A random key is fine for simulation
    let key = SigningKey::from_bytes(&[0; 32]);

    let op = Operation {
        source_account: None,
        body: stellar_xdr::curr::OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
            host_function: stellar_xdr::curr::HostFunction::InvokeContract(InvokeContractArgs {
                contract_address: ScAddress::from_str(&bstop_tkn_address)?,
                function_name: ScSymbol::try_from("get_total_supply").unwrap(),
                args: VecM::try_from(vec![])?,
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
    let sim_result = rpc.simulate_transaction(&transaction).await?;
    let contract_function_result =
        ScVal::from_xdr_base64(sim_result.results[0].xdr.clone(), Limits::none())?;
    match &contract_function_result {
        ScVal::I128(value) => return Ok(value.into()),
        _ => return Err(Error::msg("Error: failed to get total comet tokens")),
    };
}

pub fn update_rate(numerator: i128, denominator: i128) -> Result<i128> {
    let rate = numerator
        .fixed_div_floor(denominator, SCALAR_9)
        .unwrap_or(SCALAR_9 + 1);
    assert!(rate.gt(&1_000_0000));
    if rate.gt(&1_000_0000) {
        error!("Error: rate exceeds maximum value");
    }
    return Ok(rate);
}

pub async fn get_asset_prices_db(
    rpc: &Client,
    oracle_id: &String,
    oracle_decimals: &u32,
    assets: &Vec<String>,
    db_manager: &DbManager,
) -> Result<()> {
    // A random key is fine for simulation
    let key = SigningKey::from_bytes(&[0; 32]);
    // get asset prices from oracle
    for asset in assets.iter() {
        let tx_builder = BlendTxBuilder {
            contract_id: oracle_id.clone(),
            signing_key: key.clone(),
        };
        let op = tx_builder.get_last_price(asset);
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
            ScVal::from_xdr_base64(sim_result.results[0].xdr.clone(), Limits::none())?;
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
        price = price * SCALAR_7 / (10 as i128).pow(*oracle_decimals);
        db_manager.set_asset_price(asset.clone(), price)?;
    }
    Ok(())
}

pub async fn get_reserve_list(rpc: &Client, pool: &String) -> Result<Vec<String>> {
    let mut assets: Vec<String> = Vec::new();
    let reserve_list_entry = rpc.get_ledger_entries(&[stellar_xdr::curr::LedgerKey::ContractData(
        LedgerKeyContractData {
            contract: ScAddress::from_str(&pool)?,
            key: ScVal::Symbol(ScSymbol::from(ScSymbol::from(StringM::from_str("ResList")?))),
            durability: stellar_xdr::curr::ContractDataDurability::Persistent,
        },
    )])
    .await?;
    if let Some(entries) = reserve_list_entry.entries {
        if let Some(entry) = entries.get(0) {
            let value = LedgerEntryData::from_xdr_base64(entry.xdr.clone(), Limits::none())?;
            match value {
                LedgerEntryData::ContractData(data) => match data.val {
                    ScVal::Vec(vec) => {
                        vec.unwrap().iter().for_each(|entry| {
                            assets.push(decode_scaddress_to_string(entry));
                        });
                    }
                    _ => error!("Error: expected LedgerEntryData to be Vec"),
                },
                _ => error!("Error: expected LedgerEntryData to be ContractData"),
            }
        }
    }
    Ok(assets)
}

pub async fn load_reserve_configs(
    rpc: &Client,
    pool: &String,
    assets: &Vec<String>,
    db_manager: &DbManager,
) -> Result<()> {
    let mut reserve_configs: HashMap<String, HashMap<String, ReserveConfig>> = HashMap::new();
    let mut ledger_keys: Vec<LedgerKey> = Vec::new();
    for asset in assets {
        let asset_id = ScVal::Address(ScAddress::from_str(&asset)?);

        let reserve_config_key = ScVal::Vec(Some(ScVec::try_from(vec![
            ScVal::Symbol(ScSymbol::from(ScSymbol::from(StringM::from_str(
                "ResConfig",
            )?))),
            asset_id.clone(),
        ])?));
        let reserve_data_key = ScVal::Vec(Some(ScVec::try_from(vec![
            ScVal::Symbol(ScSymbol::from(ScSymbol::from(StringM::from_str(
                "ResData",
            )?))),
            asset_id,
        ])?));
        let reserve_config_ledger_key =
            stellar_xdr::curr::LedgerKey::ContractData(LedgerKeyContractData {
                contract: ScAddress::from_str(&pool)?,
                key: reserve_config_key,
                durability: stellar_xdr::curr::ContractDataDurability::Persistent,
            });
        let reserve_data_ledger_key =
            stellar_xdr::curr::LedgerKey::ContractData(LedgerKeyContractData {
                contract: ScAddress::from_str(&pool)?,
                key: reserve_data_key,
                durability: stellar_xdr::curr::ContractDataDurability::Persistent,
            });
        ledger_keys.push(reserve_config_ledger_key);
        ledger_keys.push(reserve_data_ledger_key);
    }

    let result = rpc.get_ledger_entries(&ledger_keys).await?;
    if let Some(entries) = result.entries {
        for entry in entries {
            let value = LedgerEntryData::from_xdr_base64(entry.xdr, Limits::none())?;
            match &value {
                LedgerEntryData::ContractData(data) => {
                    let key = decode_entry_key(&data.key);
                    let mut asset_id: String = Default::default();
                    match &data.key {
                        ScVal::Vec(vec) => {
                            if let Some(vec) = vec {
                                asset_id = decode_scaddress_to_string(&vec[1]);
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
                        _ => error!("Error: found unexpected key {}", key),
                    }
                    db_manager.set_reserve_config(pool, &asset_id, res_config)?;
                }
                _ => (),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::constants::{SCALAR_7, SCALAR_9};

    #[test]
    fn test_liq_pct() {
        //set up test
        let adj_liabilities_value = 125 * SCALAR_7;
        let liabilities_value = 100 * SCALAR_7;
        let adj_collateral_value = 108 * SCALAR_7;
        let collateral_value = 120 * SCALAR_7;
        let pct = super::get_liq_percent(
            adj_liabilities_value,
            liabilities_value,
            adj_collateral_value,
            collateral_value,
        );
        assert_eq!(pct, 84);
    }

    #[test]
    fn calc_position_value_collateral() {
        let config = super::ReserveConfig {
            asset: "CDMLFMKMMD7MWZP3FKUBZPVHTUEDLSX4BYGYKH4GCESXYHS3IHQ4EIG4".to_string(),
            index: 0,
            collateral_factor: 500_0000,
            liability_factor: 500_0000,
            scalar: SCALAR_9,
            est_b_rate: 1_100_000_000,
            est_d_rate: 1_100_000_000,
        };
        let price = 2 * SCALAR_7;
        let amount = 2 * SCALAR_9;
        let (raw_val, adj_val) = super::calc_position_value(config, price, amount, true);
        assert_eq!(raw_val, 4_400_0000);
        assert_eq!(adj_val, 2_200_0000);
    }
    #[test]
    fn calc_position_value_debt() {
        let config = super::ReserveConfig {
            asset: "CDMLFMKMMD7MWZP3FKUBZPVHTUEDLSX4BYGYKH4GCESXYHS3IHQ4EIG4".to_string(),
            index: 0,
            collateral_factor: 500_0000,
            liability_factor: 500_0000,
            scalar: SCALAR_9,
            est_b_rate: 1_100_000_000,
            est_d_rate: 1_100_000_000,
        };
        let price = 2 * SCALAR_7;
        let amount = 2 * SCALAR_9;
        let (raw_val, adj_val) = super::calc_position_value(config, price, amount, false);
        assert_eq!(raw_val, 4_400_0000);
        assert_eq!(adj_val, 8_800_0000);
    }
}
