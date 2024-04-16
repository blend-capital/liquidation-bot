#![warn(unused_crate_dependencies)]
#![deny(unused_must_use, rust_2018_idioms)]
#![doc(test(
    no_crate_inject,
    attr(deny(warnings, rust_2018_idioms), allow(dead_code, unused_variables))
))]
/// A strategy that fills liquidations for the Blend protocol
/// @dev: This strategy relies on a separate strategy updating prices, b and d rates, and asset reserve configs.
/// !This is handled by the blend auctioneer strategy, but if you do not wish to
/// !run that strategy you can create an alternative strategy to update prices.
/// !many user's will find doing so desireable as the auctioneer strategy tracks oracle
/// !prices, which may not accurately reflect the price of assets on the open market - especially when adjusted
/// !for relative liquidity.

/// This module contains the core strategy implementation.
pub mod strategy;

// This module manages ongoing auctions
pub mod auction_manager;
