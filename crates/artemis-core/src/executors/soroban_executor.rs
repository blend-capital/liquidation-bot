use std::{
    ops::{Div, Mul},
    sync::Arc,
};

use crate::types::Executor;
use anyhow::{Context, Result};
use async_trait::async_trait;
use ethers::{
    providers::Middleware,
    types::{transaction::eip2718::TypedTransaction, U256},
};
use soroban_client::{transaction::SimulationResponse, *};

/// An executor that sends transactions to the mempool.
pub struct SorobanExecutor {
    client: soroban_client::server::Server,
    network_passphrase: String,
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
    pub tx: soroban_client::transaction::Transaction,
    pub gas_bid_info: Option<GasBidInfo>,
    pub private_key: String,
}

impl SorobanExecutor {
    pub fn new(client: soroban_client::server::Server, network_passphrase: String) -> Self {
        Self {
            client,
            network_passphrase,
        }
    }
}

#[async_trait]
impl Executor<SubmitStellarTx> for SorobanExecutor {
    /// Send a transaction to the mempool.
    async fn execute(&self, mut action: SubmitStellarTx) -> Result<()> {
        //TODO handle gas estimate here
        let sim: soroban_client::soroban_rpc::soroban_rpc::SimulateTransactionResponse =
            self.client.simulate_transaction(action.tx.clone()).await?;
        let mut prepped_tx = soroban_client::transaction::assemble_transaction(
            action.tx.clone(),
            &self.network_passphrase,
            SimulationResponse::Normal(sim),
        )
        .unwrap()
        .build();
        prepped_tx
            .sign(&[
                soroban_client::keypair::Keypair::from_secret(action.private_key.as_str()).unwrap(),
            ]);

        let resp = self.client.send_transaction(prepped_tx);

        Ok(())
    }
}
