use std::collections::HashMap;

use crate::types::{AuctionData, ReserveConfig, UserPositions};
use ed25519_dalek::SigningKey;
use soroban_cli::rpc::Client;
use soroban_spec_tools::from_string_primitive;
use stellar_xdr::curr::{
    Hash, InvokeContractArgs, InvokeHostFunctionOp, LedgerEntryData, Limits, Memo, Operation,
    ReadXdr, ScAddress, ScSpecTypeDef, ScSymbol, ScVal, ScVec, Transaction, TransactionEnvelope,
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
    reserve_configs: &HashMap<Hash, ReserveConfig>,
) -> UserPositions {
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
                                            let mut index: u32 = 0;
                                            match entry.key {
                                                ScVal::U32(num) => {
                                                    index = num;
                                                }
                                                _ => (),
                                            }
                                            let balance = decode_i128_to_native(&entry.val);
                                            for (asset_id, config) in reserve_configs.iter() {
                                                if config.index == index {
                                                    user_positions
                                                        .liabilities
                                                        .insert(asset_id.to_owned(), balance);
                                                }
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
                                            let mut index: u32 = 0;
                                            match entry.key {
                                                ScVal::U32(num) => {
                                                    index = num;
                                                }
                                                _ => (),
                                            }
                                            let balance = decode_i128_to_native(&entry.val);
                                            for (asset_id, config) in reserve_configs.iter() {
                                                if config.index == index {
                                                    user_positions
                                                        .collateral
                                                        .insert(asset_id.to_owned(), balance);
                                                }
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
    return user_positions;
}

// computes the value of reserve assets both before and after collateral or liability factors are applied
pub fn sum_adj_asset_values(
    assets: HashMap<Hash, i128>,
    reserve_conf: &HashMap<Hash, ReserveConfig>,
    asset_prices: &HashMap<Hash, i128>,
    collateral: bool,
) -> (i128, i128) {
    let mut value: i128 = 0;
    let mut adjusted_value: i128 = 0;
    for (asset, amount) in assets.iter() {
        let config = reserve_conf.get(asset).unwrap();
        let modifiers: (i128, i128) = if collateral {
            println!("c price {} ", asset_prices.get(asset).unwrap());
            println!("b rate {}", config.est_b_rate);
            (config.est_b_rate, config.collateral_factor as i128)
        } else {
            println!("l price {} ", asset_prices.get(asset).unwrap());
            println!("d rate {}", config.est_b_rate);
            let test = asset_prices.get(asset).unwrap() * amount / config.scalar;
            println!("is ok? {}", test);
            (
                config.est_d_rate,
                1e14 as i128 / config.liability_factor as i128,
            )
        };
        let raw_val =
            asset_prices.get(asset).unwrap() * amount / config.scalar * modifiers.0 / 1e9 as i128; //oracle scalar is 7 on local, 14 on testnet
        let adj_val = raw_val * modifiers.1 / 1e7 as i128;

        value += raw_val;
        adjusted_value += adj_val;
    }
    println!("value {}", value);
    (value, adjusted_value)
}

// returns 0 if user should be ignored, 1 if user should be watched, a pct if user should be liquidated for the given pct
pub fn evaluate_user(
    reserve_configs: &HashMap<Hash, ReserveConfig>,
    asset_prices: &HashMap<Hash, i128>,
    user_positions: &UserPositions,
) -> u64 {
    let (collateral_value, adj_collateral_value) = sum_adj_asset_values(
        user_positions.collateral.clone(),
        reserve_configs,
        &asset_prices,
        true,
    );
    let (liabilities_value, adj_liabilities_value) = sum_adj_asset_values(
        user_positions.liabilities.clone(),
        reserve_configs,
        &asset_prices,
        false,
    );
    let remaining_power = adj_collateral_value - adj_liabilities_value;
    println!("adj collateral {}", adj_collateral_value);
    println!("adj liabilities {}", adj_liabilities_value);
    let mut return_val = 0;
    if adj_collateral_value == 0 && adj_liabilities_value > 0 {
        return_val = 0; //we need to do a bad debt on these guys
    } else if remaining_power > adj_liabilities_value * 5 || adj_collateral_value == 0 {
        // user's HF is over 5 so we ignore them// TODO: this might not be large enough
        // we also ignore user's with no collateral
        return_val = 1;
    } else if remaining_power > 0 {
        return_val = 2; // User's cooling but we still wanna track
    } else {
        const SCL_7: i128 = 1e7 as i128;
        let inv_lf = adj_liabilities_value * SCL_7 / liabilities_value;
        let cf = adj_collateral_value * SCL_7 / collateral_value;
        let numerator = adj_liabilities_value * 1_100_0000 / SCL_7 - adj_collateral_value;
        let est_incentive = SCL_7 + (SCL_7 - cf * SCL_7 / inv_lf) / 2;
        let denominator = inv_lf * 1_100_0000 / SCL_7 - cf * est_incentive / SCL_7;
        let mut pct = 0;
        if denominator != 0 && liabilities_value != 0 {
            pct = numerator * SCL_7 / denominator * 100 / liabilities_value;
        }
        println!("liabilities {}", liabilities_value);
        println!("pct {}", pct);
        if pct < 0 {
            panic!("negative liq pct")
        };
        return_val = if pct > 100 { 100 } else { pct as u64 };
    }
    println!("return val {}", return_val);
    return_val
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
    let mut usdc_out: i128 = 0;
    match &contract_function_result {
        ScVal::I128(value) => {
            usdc_out = value.into();
        }
        _ => (),
    }
    println!("usdc out {}", usdc_out);
    return Ok(usdc_out);
}
