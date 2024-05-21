use std::path::{Path, PathBuf};

use crate::types::{AuctionData, ReserveConfig};
use anyhow::Result;
use rusqlite::{params, Connection};
use tracing::{error, info};

#[derive(Debug, Clone)]
pub struct DbManager {
    pub db_directory: String,
    pub blend_asset_path: PathBuf,
    pub blend_users_path: PathBuf,
    pub filled_auctions_path: PathBuf,
}

impl DbManager {
    pub fn new(db_path: String) -> Self {
        DbManager {
            db_directory: db_path.clone(),
            blend_asset_path: Path::new(&db_path).join("blend_assets.db").to_path_buf(),
            blend_users_path: Path::new(&db_path).join("blend_users.db").to_path_buf(),
            filled_auctions_path: Path::new(&db_path).join("filled_auctions.db").to_path_buf(),
        }
    }

    pub fn initialize(&self, assets: &Vec<String>) -> Result<()> {
        let db = Connection::open(&self.blend_asset_path).unwrap();
        db.execute(
            "CREATE table if not exists asset_prices (
                address string primary key,
                price integer not null
             )",
            [],
        )?;
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

        let placeholder_int = 1i64;
        for asset in assets.clone() {
            let result = db.execute(
                "INSERT INTO asset_prices (address, price) VALUES (?, ?)",
                params![asset, placeholder_int],
            );
            match result {
                Ok(_) => (),
                // Asset already exists in the table ignore error
                Err(rusqlite::Error::SqliteFailure(err, _))
                    if err.code == rusqlite::ErrorCode::ConstraintViolation => {}
                Err(err) => error!("Error inserting price: {}", err),
            }
        }
        db.close().unwrap();

        let db = Connection::open(&self.blend_users_path).unwrap();
        db.execute(
            "create table if not exists users (
            id integer primary key,
            address string not null unique
         )",
            [],
        )?;
        db.close().unwrap();
        let db = Connection::open(&self.filled_auctions_path).unwrap();

        db.execute(
            "create table if not exists filled_auctions (
            id integer primary key,
            fill_block integer not null,
            lot_assets string not null,
            lot_amounts string not null,
            bid_assets string not null
            bid_amounts string not null
            percent_filled integer not null
         )",
            [],
        )?;
        db.close().unwrap();
        Ok(())
    }

    pub fn set_asset_price(&self, asset_id: String, price: i128) -> Result<()> {
        let db = Connection::open(&self.blend_asset_path).unwrap();
        db.execute(
            "UPDATE asset_prices SET price = ?2 WHERE address = ?1",
            [asset_id, price.to_string()],
        )?;
        db.close().unwrap();
        Ok(())
    }

    pub fn get_asset_price(&self, asset_id: &String) -> Result<i128, rusqlite::Error> {
        let db = Connection::open(&self.blend_asset_path).unwrap();
        let price_result = db.query_row(
            "SELECT price FROM asset_prices WHERE address = ?",
            [asset_id],
            |row| row.get::<_, isize>(0),
        )?;
        db.close().unwrap();
        Ok(price_result as i128)
    }

    pub fn get_reserve_config_from_asset(
        &self,
        pool: &String,
        asset: &String,
    ) -> Result<ReserveConfig> {
        let db = Connection::open(&self.blend_asset_path).unwrap();
        let result = db.query_row(
            "SELECT asset_index, dRate,
            bRate,
            collateralFactor,
            liabilityFactor,
            scalar FROM pool_asset_data WHERE key = ?",
            [asset.clone() + pool],
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
        )?;
        Ok(result)
    }

    pub fn get_reserve_config_from_index(
        &self,
        pool: &String,
        index: &u32,
    ) -> Result<ReserveConfig> {
        let db = Connection::open(&self.blend_asset_path).unwrap();
        let result = db.query_row(
            "SELECT address, dRate,
            bRate,
            collateralFactor,
            liabilityFactor,
            scalar FROM pool_asset_data WHERE asset_index = ?1 AND pool_address = ?2",
            params![index, pool,],
            |row| {
                Ok(ReserveConfig {
                    asset: row.get::<_, String>(0)?,
                    index: *index,
                    est_d_rate: row.get::<_, isize>(1)? as i128,
                    est_b_rate: row.get::<_, isize>(2)? as i128,
                    collateral_factor: row.get::<_, u32>(3)?,
                    liability_factor: row.get::<_, u32>(4)?,
                    scalar: row.get::<_, isize>(5)? as i128,
                })
            },
        )?;
        Ok(result)
    }

    pub fn set_reserve_config(
        &self,
        pool: &String,
        asset: &String,
        config: &ReserveConfig,
    ) -> Result<()> {
        let db = Connection::open(&self.blend_asset_path).unwrap();

        let key = asset.clone() + pool;
        db.execute(
            "INSERT OR REPLACE INTO pool_asset_data (key, bRate, dRate, asset_index, collateralFactor, liabilityFactor, scalar, pool_address, address) VALUES (?7, ?1, ?2, ?3, ?4, ?5, ?6, ?8, ?9)",
            params![
                config.est_b_rate as u64,
                config.est_d_rate as u64,
                config.index,
                config.collateral_factor,
                config.liability_factor,
                config.scalar as u64,
                key,
                pool,
                asset,
            ],
        )?;
        db.close().unwrap();
        Ok(())
    }

    pub fn update_reserve_config_rate(
        &self,
        pool_id: &String,
        asset_id: &String,
        rate: i128,
        // b_rate: true if updating b_rate, false if updating d_rate
        rate_type: bool,
    ) -> Result<()> {
        let db = Connection::open(&self.blend_asset_path).unwrap();
        let key = asset_id.clone() + &pool_id.clone();
        if rate_type {
            db.execute(
                "UPDATE pool_asset_data SET bRate = ?1 WHERE key = ?2",
                params![rate as u64, key,],
            )?
        } else {
            db.execute(
                "UPDATE pool_asset_data SET dRate = ?1 WHERE key = ?2",
                params![rate as u64, key,],
            )?
        };
        db.close().unwrap();
        Ok(())
    }

    pub fn get_users(&self) -> Result<Vec<String>, rusqlite::Error> {
        let db = Connection::open(&self.blend_users_path).unwrap();
        let mut user_addresses = Vec::new();

        {
            let mut stmt = db.prepare("SELECT address FROM users")?;
            let users = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;
            for user in users {
                user_addresses.push(match stellar_strkey::Strkey::from_string(&user?).unwrap() {
                    stellar_strkey::Strkey::PublicKeyEd25519(pk) => pk.to_string(),
                    stellar_strkey::Strkey::Contract(pk) => pk.to_string(),
                    _ => continue,
                });
            }
        }
        db.close().unwrap();
        return Ok(user_addresses);
    }
    pub fn add_user(&self, user_id: &String) -> Result<()> {
        let db = Connection::open(Path::new(&self.blend_users_path))?;

        match db.execute("INSERT INTO users (address) VALUES (?1)", [user_id.clone()]) {
            Ok(_) => {
                info!("Found new user: {}", user_id.clone());
            }
            Err(_) => {}
        }
        db.close().unwrap();
        Ok(())
    }
    pub fn add_auction(
        &self,
        auction_data: &AuctionData,
        fill_block: u32,
        percent_filled: i128,
    ) -> Result<()> {
        let db = Connection::open(Path::new(&self.filled_auctions_path))?;
        let lot_assets = auction_data
            .lot
            .keys()
            .map(|key| key.to_string())
            .collect::<Vec<String>>()
            .join(",");
        let lot_values = auction_data
            .lot
            .values()
            .map(|value| value.to_string())
            .collect::<Vec<String>>()
            .join(",");
        let bid_assets = auction_data
            .bid
            .keys()
            .map(|key| key.to_string())
            .collect::<Vec<String>>()
            .join(",");
        let bid_values = auction_data
            .bid
            .values()
            .map(|value| value.to_string())
            .collect::<Vec<String>>()
            .join(",");

        match db.execute(
            "INSERT INTO filled_auctions (block,lot_assets,lot_amounts,bid_assets,bid_amounts,percent_filled) VALUES (?1,?2,?3,?4,?5,?6)",
            [
                fill_block.to_string(),
                lot_assets,
                lot_values,
                bid_assets,
                bid_values,
                percent_filled.to_string(),
            ],
        ) {
            Ok(_) => {
                info!("Stored new fill on block: {}", fill_block.clone());
            }
            Err(_) => {}
        }
        db.close().unwrap();
        Ok(())
    }
}
