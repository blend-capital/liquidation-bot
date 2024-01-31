use std::collections::HashMap;

use crate::types::{AuctionData, ReserveConfig, UserPositions};
use stellar_xdr::curr::{Hash, LedgerEntryData, ScAddress, ScVal};
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
//Returns (index, collateral_factor, liability_factor)
pub fn reserve_config_from_ledger_entry(ledger_entry_data: &LedgerEntryData) -> (u32, u32, u32) {
    let mut collateral_factor: u32 = 0;
    let mut liability_factor: u32 = 0;
    let mut index: u32 = 0;
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
                            _ => (),
                        }
                    }
                }
            }
            _ => (),
        },
        _ => println!("Error: expected LedgerEntryData to be ContractData"),
    }
    return (index, collateral_factor, liability_factor);
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
                                            let mut balance: i128 = 0;
                                            match entry.key {
                                                ScVal::U32(num) => {
                                                    index = num;
                                                }
                                                _ => (),
                                            }
                                            balance = decode_i128_to_native(&entry.val);
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
                                            let mut balance: i128 = 0;
                                            match entry.key {
                                                ScVal::U32(num) => {
                                                    index = num;
                                                }
                                                _ => (),
                                            }
                                            balance = decode_i128_to_native(&entry.val);
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
            (config.est_b_rate, config.collateral_factor as i128)
        } else {
            (config.est_d_rate, config.liability_factor as i128)
        };
        let raw_val = amount * modifiers.0 * asset_prices.get(asset).unwrap() / 1e16 as i128;
        let adj_val = raw_val * modifiers.1 / 1e7 as i128;

        value += raw_val;
        adjusted_value += adj_val;
    }
    (value, adjusted_value)
}
