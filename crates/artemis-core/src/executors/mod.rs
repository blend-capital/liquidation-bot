//! Executors are responsible for taking actions produced by strategies and
//! executing them in different domains. For example, an executor might take a
//! `SubmitTx` action and submit it to the mempool.

/// This executor submits transactions to the flashbots relay.
pub mod flashbots_executor;

/// This executor submits transactions to stellar.
pub mod soroban_executor;

/// This executor submits bundles to the flashbots matchmaker.
pub mod mev_share_executor;

/// mem pool executor
pub mod mempool_executor;
