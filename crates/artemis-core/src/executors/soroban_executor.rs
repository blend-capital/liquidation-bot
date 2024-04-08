use crate::types::Executor;
use anyhow::Result;
use async_trait::async_trait;
use ed25519_dalek::SigningKey;
use soroban_rpc::Client;
use std::vec;
use stellar_strkey::{ed25519::PublicKey as Ed25519PublicKey, Strkey};
use stellar_xdr::curr::{Memo, Operation, Preconditions, Transaction, Uint256};
use tracing::{error, info};

/// An executor that sends transactions to the mempool.
pub struct SorobanExecutor {
    network_passphrase: String,
    rpc: Client,
}

/// Information about the gas bid for a transaction.
#[derive(Debug, Clone)]
pub struct GasBidInfo {
    /// Total profit expected from opportunity in XLM
    pub total_profit: i128,
    /// Percentage of bid profit to use for gas
    pub bid_percentage: u64,
}

#[derive(Debug, Clone)]
pub struct SubmitStellarTx {
    pub op: Operation,
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
        let mut seq_num = self
            .rpc
            .get_account(
                &Strkey::PublicKeyEd25519(Ed25519PublicKey(
                    action.signing_key.verifying_key().to_bytes(),
                ))
                .to_string(),
            )
            .await
            .unwrap()
            .seq_num
            .into();
        seq_num += 1;
        let fee = match action.gas_bid_info {
            Some(gas_bid_info) => {
                (gas_bid_info.total_profit * gas_bid_info.bid_percentage as i128 / 100) as u32
            }
            None => 10000,
        };
        let tx = Transaction {
            source_account: stellar_xdr::curr::MuxedAccount::Ed25519(Uint256(
                action.signing_key.verifying_key().to_bytes(),
            )),
            fee,
            seq_num: stellar_xdr::curr::SequenceNumber(seq_num),
            cond: Preconditions::None,
            memo: Memo::None,
            operations: vec![action.op.clone()].try_into()?,
            ext: stellar_xdr::curr::TransactionExt::V0,
        };
        info!("Submitting tx: {:?}", action.op.body.clone());
        let res = self
            .rpc
            .prepare_and_send_transaction(
                &tx,
                &action.signing_key.clone(),
                &[action.signing_key],
                &self.network_passphrase,
                None,
                None,
            )
            .await;
        match res {
            Ok(res) => {
                info!("Soroban response: {:?}", res.status)
            }
            Err(e) => error!("Soroban error: {}", e),
        };
        Ok(())
    }
}
