use std::collections::HashMap;

use artemis_core::{
    collectors::block_collector::NewBlock, executors::soroban_executor::SubmitStellarTx,
};

use stellar_strkey::Strkey;

use soroban_cli::rpc::Event as SorobanEvent;
use stellar_xdr::curr::Hash;
/// Core Event enum for the current strategy.
#[derive(Debug, Clone)]
pub enum Event {
    SorobanEvents(Box<SorobanEvent>),
    NewBlock(Box<NewBlock>),
}

/// Core Action enum for the current strategy.
#[derive(Debug, Clone)]
pub enum Action {
    SubmitTx(SubmitStellarTx),
}

/// Configuration for variables we need to pass to the strategy.
#[derive(Debug, Clone)]
pub struct Config {
    pub rpc_url: String,
    pub pools: Vec<Hash>,
    pub assets: Vec<Hash>,
    pub bid_percentage: u64,
    pub oracle_id: Hash,
    pub us: String,
    pub min_hf: i128,
    pub required_profit: i128,
}
#[derive(Debug, Clone)]
pub struct PendingFill {
    pub pool: Hash,
    pub user: Hash,
    pub collateral: HashMap<Hash, i128>,
    pub liabilities: HashMap<Hash, i128>,
    pub pct_filled: u64,
    pub target_block: u32,
    pub auction_type: u8,
}
#[derive(Debug, Clone)]
pub struct UserPositions {
    pub collateral: HashMap<Hash, i128>,
    pub liabilities: HashMap<Hash, i128>,
}

#[derive(Debug, Clone)]
pub struct ReserveConfig {
    pub index: u32,
    pub liability_factor: u32,
    pub collateral_factor: u32,
    pub est_b_rate: i128,
    pub est_d_rate: i128,
}

#[derive(Debug, Clone)]
pub struct AuctionData {
    pub bid: HashMap<Hash, i128>, //liabilities || backstop_token || bad_debt
    pub lot: HashMap<Hash, i128>, //collateral || interest || bad_debt
    pub block: u32,
}

// /// Convenience function to convert a hash to a fulfill listing request
// pub fn hash_to_fulfill_listing_request(hash: H256) -> FulfillListingRequest {
//     FulfillListingRequest {
//         listing: Listing {
//             hash,
//             chain: Chain::Mainnet,
//             protocol_version: ProtocolVersion::V1_5,
//         },
//         fulfiller: Fulfiller {
//             address: H160::zero(),
//         },
//     }
// }

// /// Convenience function to convert a fulfill listing response to basic order parameters
// pub fn fulfill_listing_response_to_basic_order_parameters(
//     val: FulfillListingResponse,
// ) -> BasicOrderParameters {
//     let params = val.fulfillment_data.transaction.input_data.parameters;

//     let recipients: Vec<AdditionalRecipient> = params
//         .additional_recipients
//         .iter()
//         .map(|ar| AdditionalRecipient {
//             recipient: ar.recipient,
//             amount: ar.amount,
//         })
//         .collect();

//     BasicOrderParameters {
//         consideration_token: params.consideration_token,
//         consideration_identifier: params.consideration_identifier,
//         consideration_amount: params.consideration_amount,
//         offerer: params.offerer,
//         zone: params.zone,
//         offer_token: params.offer_token,
//         offer_identifier: params.offer_identifier,
//         offer_amount: params.offer_amount,
//         basic_order_type: params.basic_order_type,
//         start_time: params.start_time,
//         end_time: params.end_time,
//         zone_hash: params.zone_hash.into(),
//         salt: params.salt,
//         offerer_conduit_key: params.offerer_conduit_key.into(),
//         fulfiller_conduit_key: params.fulfiller_conduit_key.into(),
//         total_original_additional_recipients: params.total_original_additional_recipients,
//         additional_recipients: recipients,
//         signature: params.signature,
//     }
// }
