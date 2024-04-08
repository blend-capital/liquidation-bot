#![warn(unused_crate_dependencies)]
#![deny(unused_must_use, rust_2018_idioms)]
#![doc(test(
    no_crate_inject,
    attr(deny(warnings, rust_2018_idioms), allow(dead_code, unused_variables))
))]
///! A collection of utilities for Blend strategies
// This module contains helpers used in blend strategies
pub mod helper;
/// This module builds transactions for blend strategies
pub mod transaction_builder;
/// This module contains types used in blend strategies
pub mod types;

pub mod constants;
