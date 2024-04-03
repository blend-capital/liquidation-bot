use std::collections::HashMap;

use artemis_core::{
    collectors::block_collector::NewBlock, executors::soroban_executor::SubmitStellarTx,
};
use rusqlite::{params, Connection};
use serde::{Deserialize, Deserializer};
use soroban_cli::utils::contract_id_from_str;
use soroban_rpc::Event as SorobanEvent;
use stellar_xdr::curr::{Hash, ScAddress};
/// Core Event enum for the current strategy.
#[derive(Debug, Clone)]
pub enum Event {
    SorobanEvents(Box<SorobanEvent>),
    NewBlock(Box<NewBlock>),
}

/// Core Action enum for the current strategy.
#[derive(Debug, Clone)]
pub enum Action {
    SubmitTx(SubmitStellarTx),
}

/// Configuration for variables we need to pass to the strategy.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub rpc_url: String,
    #[serde(deserialize_with = "from_strkey_vec")]
    pub pools: Vec<Hash>,
    #[serde(deserialize_with = "from_strkey_vec")]
    pub assets: Vec<Hash>,
    #[serde(deserialize_with = "from_strkey")]
    pub backstop: Hash,
    #[serde(deserialize_with = "from_strkey")]
    pub backstop_token_address: Hash,
    #[serde(deserialize_with = "from_strkey")]
    pub usdc_token_address: Hash,
    pub bid_percentage: u64,
    #[serde(deserialize_with = "from_strkey")]
    pub oracle_id: Hash,
    pub min_hf: i128,
    pub required_profit: i128,
    pub network_passphrase: String,
    pub oracle_decimals: u32,
}
fn from_strkey_vec<'de, D>(deserializer: D) -> Result<Vec<Hash>, D::Error>
where
    D: Deserializer<'de>,
{
    let string_vec: Vec<String> = Deserialize::deserialize(deserializer)?;
    // do better hex decoding than this
    let mut hash_vec: Vec<Hash> = Vec::new();
    for s in string_vec {
        hash_vec.push(Hash(contract_id_from_str(&s).unwrap()));
    }
    Ok(hash_vec)
}
fn from_strkey<'de, D>(deserializer: D) -> Result<Hash, D::Error>
where
    D: Deserializer<'de>,
{
    let string: &str = Deserialize::deserialize(deserializer)?;
    Ok(Hash(contract_id_from_str(string).unwrap()))
}
#[derive(Debug, Clone, Deserialize)]
pub struct PendingFill {
    pub pool: Hash,
    pub user: Hash,
    pub collateral: HashMap<Hash, i128>,
    pub liabilities: HashMap<Hash, i128>,
    pub pct_filled: u64,
    pub target_block: u32,
    pub auction_type: u8,
}
#[derive(Debug, Clone)]
pub struct UserPositions {
    pub collateral: HashMap<Hash, i128>,
    pub liabilities: HashMap<Hash, i128>,
}

#[derive(Debug, Clone)]
pub struct ReserveConfig {
    pub asset: Hash,
    pub index: u32,
    pub liability_factor: u32,
    pub collateral_factor: u32,
    pub est_b_rate: i128,
    pub est_d_rate: i128,
    pub scalar: i128,
}
impl ReserveConfig {
    pub fn default(asset: Hash) -> Self {
        ReserveConfig {
            asset,
            index: Default::default(),
            liability_factor: Default::default(),
            collateral_factor: Default::default(),
            est_b_rate: Default::default(),
            est_d_rate: Default::default(),
            scalar: Default::default(),
        }
    }

    pub fn new(
        asset: Hash,
        index: u32,
        liability_factor: u32,
        collateral_factor: u32,
        est_b_rate: i128,
        est_d_rate: i128,
        scalar: i128,
    ) -> Self {
        ReserveConfig {
            asset,
            index,
            liability_factor,
            collateral_factor,
            est_b_rate,
            est_d_rate,
            scalar,
        }
    }
    pub fn from_db_w_asset(
        pool: &Hash,
        asset: &Hash,
        db: &Connection,
    ) -> Result<Self, rusqlite::Error> {
        db.query_row(
            "SELECT asset_index, dRate,
            bRate,
            collateralFactor,
            liabilityFactor,
            scalar FROM pool_asset_data WHERE key = ?",
            [(ScAddress::Contract(asset.clone()).to_string()
                + &ScAddress::Contract(pool.clone()).to_string())
                .to_string()],
            |row| {
                Ok(ReserveConfig {
                    asset: asset.clone(),
                    index: row.get::<_, u32>(0)?,
                    est_d_rate: row.get::<_, isize>(1)? as i128,
                    est_b_rate: row.get::<_, isize>(2)? as i128,
                    collateral_factor: row.get::<_, u32>(3)?,
                    liability_factor: row.get::<_, u32>(4)?,
                    scalar: row.get::<_, isize>(5)? as i128,
                })
            },
        )
    }
    pub fn from_db_w_index(
        pool: &Hash,
        index: &u32,
        db: &Connection,
    ) -> Result<Self, rusqlite::Error> {
        db.query_row(
            "SELECT address, dRate,
            bRate,
            collateralFactor,
            liabilityFactor,
            scalar FROM pool_asset_data WHERE asset_index = ?1 AND pool_address = ?2",
            params![index, ScAddress::Contract(pool.clone()).to_string(),],
            |row| {
                Ok(ReserveConfig {
                    asset: Hash(contract_id_from_str(&row.get::<_, String>(0)?).unwrap()),
                    index: *index,
                    est_d_rate: row.get::<_, isize>(1)? as i128,
                    est_b_rate: row.get::<_, isize>(2)? as i128,
                    collateral_factor: row.get::<_, u32>(3)?,
                    liability_factor: row.get::<_, u32>(4)?,
                    scalar: row.get::<_, isize>(5)? as i128,
                })
            },
        )
    }
}

#[derive(Debug, Clone)]
pub struct AuctionData {
    pub bid: HashMap<Hash, i128>, //liabilities || backstop_token || bad_debt
    pub lot: HashMap<Hash, i128>, //collateral || interest || bad_debt
    pub block: u32,
}
