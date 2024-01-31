use std::{
    ops::{Div, Mul},
    sync::Arc,
};

use crate::types::Executor;
use anyhow::{Context, Result};
use async_trait::async_trait;
use ed25519_dalek::{SecretKey, SigningKey};
use ethers::{
    providers::Middleware,
    types::{transaction::eip2718::TypedTransaction, U256},
};
use soroban_cli::rpc::{self, Client};
use stellar_xdr::curr::{Hash, Transaction};
/// An executor that sends transactions to the mempool.
pub struct SorobanExecutor {
    // client: soroban_client::server::Server,
    network_passphrase: String,
    rpc_url: String,
    rpc: Client,
    signer: SigningKey,
}

/// Information about the gas bid for a transaction.
#[derive(Debug, Clone)]
pub struct GasBidInfo {
    /// Total profit expected from opportunity
    pub total_profit: U256,

    /// Percentage of bid profit to use for gas
    pub bid_percentage: u64,
}

#[derive(Debug, Clone)]
pub struct SubmitStellarTx {
    pub tx: stellar_xdr::curr::Transaction,
    pub gas_bid_info: Option<GasBidInfo>,
}

impl SorobanExecutor {
    pub fn new(rpc_url: &str, network_passphrase: &str, secret_key: Hash) -> Self {
        Self {
            rpc: Client::new(rpc_url).unwrap(),
            rpc_url: rpc_url.to_string(),
            network_passphrase: network_passphrase.to_string(),
            signer: SigningKey::from_bytes(&secret_key.0),
        }
    }
}

// #[async_trait]
// impl Executor<SubmitStellarTx> for SorobanExecutor {
//     /// Send a transaction to the mempool.
//     async fn execute(&self, mut action: SubmitStellarTx) -> Result<()> {
//         //TODO handle gas estimate here
//         let sim: soroban_client::soroban_rpc::soroban_rpc::SimulateTransactionResponse =
//             self.client.simulate_transaction(action.tx.clone()).await?;
//         let mut prepped_tx = soroban_client::transaction::assemble_transaction(
//             action.tx.clone(),
//             &self.network_passphrase,
//             SimulationResponse::Normal(sim),
//         )
//         .unwrap()
//         .build();
//         prepped_tx
//             .sign(&[
//                 soroban_client::keypair::Keypair::from_secret(action.private_key.as_str()).unwrap(),
//             ]);

//         let resp = self.client.send_transaction(prepped_tx);

//         Ok(())
//     }
// }
