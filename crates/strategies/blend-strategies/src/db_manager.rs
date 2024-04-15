use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{params, Connection};
use soroban_cli::utils::contract_id_from_str;
use stellar_xdr::curr::{AccountId, Hash, PublicKey, ScAddress, Uint256};
use tracing::{error, info};

use crate::types::ReserveConfig;

#[derive(Debug, Clone)]
pub struct DbManager {
    pub db_directory: String,
    pub blend_asset_path: PathBuf,
    pub blend_users_path: PathBuf,
}

impl DbManager {
    pub fn new(db_path: String) -> Self {
        DbManager {
            db_directory: db_path.clone(),
            blend_asset_path: Path::new(&db_path).join("blend_assets.db").to_path_buf(),
            blend_users_path: Path::new(&db_path).join("blend_users.db").to_path_buf(),
        }
    }

    pub fn initialize(&self, assets: &Vec<Hash>) -> Result<()> {
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
            let asset_str = ScAddress::Contract(asset).to_string();
            let result = db.execute(
                "INSERT INTO asset_prices (address, price) VALUES (?, ?)",
                params![asset_str, placeholder_int],
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
        Ok(())
    }

    pub fn set_asset_price(&self, asset_id: Hash, price: i128) -> Result<()> {
        let db = Connection::open(&self.blend_asset_path).unwrap();
        db.execute(
            "UPDATE asset_prices SET price = ?2 WHERE address = ?1",
            [
                ScAddress::Contract(asset_id.clone()).to_string(),
                price.to_string(),
            ],
        )?;
        db.close().unwrap();
        Ok(())
    }

    pub fn get_asset_price(&self, asset_id: &Hash) -> Result<i128, rusqlite::Error> {
        let db = Connection::open(&self.blend_asset_path).unwrap();
        let price_result = db.query_row(
            "SELECT price FROM asset_prices WHERE address = ?",
            [ScAddress::Contract(asset_id.clone()).to_string()],
            |row| row.get::<_, isize>(0),
        )?;
        db.close().unwrap();
        Ok(price_result as i128)
    }

    pub fn get_reserve_config_from_asset(
        &self,
        pool: &Hash,
        asset: &Hash,
    ) -> Result<ReserveConfig> {
        let db = Connection::open(&self.blend_asset_path).unwrap();
        let result = db.query_row(
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
        )?;
        Ok(result)
    }

    pub fn get_reserve_config_from_index(&self, pool: &Hash, index: &u32) -> Result<ReserveConfig> {
        let db = Connection::open(&self.blend_asset_path).unwrap();
        let result = db.query_row(
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
        )?;
        Ok(result)
    }

    pub fn set_reserve_config(
        &self,
        pool: &Hash,
        asset: &Hash,
        config: &ReserveConfig,
    ) -> Result<()> {
        let db = Connection::open(&self.blend_asset_path).unwrap();
        let pool_address_str = ScAddress::Contract(pool.clone()).to_string();
        let asset_address_str = ScAddress::Contract(asset.clone()).to_string();
        let key = (asset_address_str.clone() + &pool_address_str.clone()).to_string();
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
                pool_address_str,
                asset_address_str,
            ],
        )?;
        db.close().unwrap();
        Ok(())
    }

    pub fn update_reserve_config_rate(
        &self,
        pool_id: &Hash,
        asset_id: &Hash,
        rate: i128,
        // b_rate: true if updating b_rate, false if updating d_rate
        rate_type: bool,
    ) -> Result<()> {
        let db = Connection::open(&self.blend_asset_path).unwrap();
        let key = (ScAddress::Contract(asset_id.clone()).to_string()
            + &ScAddress::Contract(pool_id.clone()).to_string())
            .to_string();
        if rate_type {
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
        db.close().unwrap();
        Ok(())
    }

    pub fn get_users(&self) -> Result<Vec<Hash>, rusqlite::Error> {
        let db = Connection::open(&self.blend_users_path).unwrap();
        let mut user_hashes = Vec::new();

        {
            let mut stmt = db.prepare("SELECT address FROM users").unwrap();
            let users = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;
            for user in users {
                user_hashes.push(Hash(
                    stellar_strkey::ed25519::PublicKey::from_string(&user?)
                        .unwrap()
                        .0,
                ));
            }
        }
        db.close().unwrap();
        return Ok(user_hashes);
    }
    pub fn add_user(&self, user_id: &Hash) -> Result<()> {
        let db = Connection::open(Path::new(&self.blend_users_path))?;
        let public_key = ScAddress::Account(AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(
            user_id.0,
        ))))
        .to_string();

        match db.execute(
            "INSERT INTO users (address) VALUES (?1)",
            [public_key.clone()],
        ) {
            Ok(_) => {
                info!("Found new user: {}", public_key.clone());
            }
            Err(_) => {}
        }
        db.close().unwrap();
        Ok(())
    }
}
