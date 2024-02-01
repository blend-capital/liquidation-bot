use crate::types::Executor;
use anyhow::Result;
use async_trait::async_trait;
use ed25519_dalek::SigningKey;

use soroban_cli::rpc::Client;

use stellar_xdr::curr::Transaction;

/// An executor that sends transactions to the mempool.
pub struct SorobanExecutor {
    // client: soroban_client::server::Server,
    network_passphrase: String,
    rpc: Client,
}

/// Information about the gas bid for a transaction.
#[derive(Debug, Clone)]
pub struct GasBidInfo {
    /// Total profit expected from opportunity
    pub total_profit: i128,
    /// Percentage of bid profit to use for gas
    pub bid_percentage: u64,
}

#[derive(Debug, Clone)]
pub struct SubmitStellarTx {
    pub tx: Transaction,
    pub gas_bid_info: Option<GasBidInfo>,
    pub signing_key: SigningKey,
}

impl SorobanExecutor {
    pub async fn new(rpc_url: &str, network_passphrase: &str) -> Self {
        Self {
            rpc: Client::new(rpc_url).unwrap(),
            network_passphrase: network_passphrase.to_string(),
        }
    }
}

#[async_trait]
impl Executor<SubmitStellarTx> for SorobanExecutor {
    /// Send a transaction to the mempool.
    async fn execute(&self, action: SubmitStellarTx) -> Result<()> {
        // TODO: estimate gas and set fees here
        self.rpc
            .prepare_and_send_transaction(
                &action.tx,
                &action.signing_key.clone(),
                &[action.signing_key],
                &self.network_passphrase,
                None,
                None,
            )
            .await?;
        Ok(())
    }
}
