#![warn(unused_crate_dependencies)]
#![deny(unused_must_use, rust_2018_idioms)]
#![doc(test(
    no_crate_inject,
    attr(deny(warnings, rust_2018_idioms), allow(dead_code, unused_variables))
))]
//! A strategy that creates liquidation and bad debt auctions for the Blend protocol on Stellar.
//! We track user's, pool configurations, and asset prices, and create new liquidation auctions
//! whenever we find a potential liquidation

pub mod auction_manager;
/// This module contains the core strategy implementation.
pub mod auctioneer_strategy;
pub mod constants;
pub mod db_manager;
mod error_logger;
pub mod helper;
pub mod liquidation_strategy;
pub mod transaction_builder;
pub mod types;
