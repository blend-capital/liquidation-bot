use anyhow::Result;
use artemis_core::collectors::block_collector::BlockCollector;
use artemis_core::collectors::log_collector::{EventFilter, LogCollector};
use artemis_core::executors::soroban_executor::SorobanExecutor;
use blend_auctioneer::strategy::BlendAuctioneer;
use blend_liquidator::strategy::BlendLiquidator;
use blend_utilities::contracts::ContractAddress;
use blend_utilities::types::Config;
use clap::Parser;

// use artemis_core::collectors::block_collector::BlockCollector;
// use artemis_core::executors::soroban_executor::SorobanExecutor;
use blend_utilities::types::{Action, Event};
use soroban_rpc::EventType;
use stellar_xdr::curr::Hash;
// use opensea_sudo_arb::strategy::OpenseaSudoArb;
use tracing::{info, Level};
use tracing_subscriber::{filter, prelude::*};

use artemis_core::engine::Engine;
use artemis_core::types::{CollectorMap, ExecutorMap};
use soroban_cli::utils::contract_id_from_str;
use std::env;
/// CLI Options.
#[derive(Parser, Debug)]
pub struct Args {
    // /// Ethereum node WS endpoint.
    // #[arg(long)]
    // pub wss: String,

    // /// Key for the OpenSea API.
    // #[arg(long)]
    // pub opensea_api_key: String,
    /// Private key for sending txs.
    #[arg(long)]
    pub private_key: String,

    // /// Address of the arb contract.
    // #[arg(long)]
    // pub arb_contract_address: String,

    // /// Percentage of profit to pay in gas.
    // #[arg(long)]
    // pub bid_percentage: u64,
    /// Private key for sending txs.
    #[arg(long)]
    pub network: i32,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Set up tracing and parse args.
    let filter = filter::Targets::new()
        .with_target("opensea_sudo_arb", Level::INFO)
        .with_target("artemis_core", Level::INFO);
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();

    let args = Args::parse(); //at some point pull network from arg enum
    let network = match args.network {
        0 => "http://localhost:8000".to_string(),
        1 => "http://host.docker.internal:8000".to_string(),
        2 => "https://soroban.stellar.org".to_string(),
        _ => "https://soroban-testnet.stellar.org".to_string(),
    };
    let passphrase = match args.network {
        0 => "Test SDF Network ; September 2015".to_string(),
        1 => "Test SDF Network ; September 2015".to_string(),
        2 => "Public Global Stellar Network ; September 2015".to_string(),
        _ => "Test SDF Network ; September 2015".to_string(),
    };

    // Set up engine.
    let mut engine: Engine<Event, Action> = Engine::default();
    // Set up log collector
    let log_collector = Box::new(LogCollector::new(
        network.clone(),
        EventFilter {
            event_type: EventType::Contract,
            contract_ids: vec![
                ContractAddress::get_stellar_pool(args.network).to_string(), //stellar pool
                ContractAddress::get_bridge_pool(args.network).to_string(),  //bridge pool
                ContractAddress::get_oracle(args.network).to_string(),       //oracle
            ],
            topics: vec![],
        },
    ));
    let log_collector = CollectorMap::new(log_collector, |e| Event::SorobanEvents(Box::new(e)));
    engine.add_collector(Box::new(log_collector));

    // Set up block collector.
    let block_collector = Box::new(BlockCollector::new(network.clone()));

    let block_collector = CollectorMap::new(block_collector, |e| Event::NewBlock(Box::new(e)));
    engine.add_collector(Box::new(block_collector));

    // Set up Blend Liquidator.
    let config = Config {
        rpc_url: network.clone(),
        pools: vec![
            Hash(
                contract_id_from_str(ContractAddress::get_stellar_pool(args.network)).unwrap(), //Stellar pool
            ),
            Hash(
                contract_id_from_str(ContractAddress::get_bridge_pool(args.network)).unwrap(), //Bridge pool
            ),
        ],
        assets: vec![
            Hash(contract_id_from_str(ContractAddress::get_usdc(args.network)).unwrap()), //USDC
            Hash(
                contract_id_from_str(ContractAddress::get_xlm(args.network)).unwrap(), //XLM
            ),
            Hash(
                contract_id_from_str(ContractAddress::get_btc(args.network)).unwrap(), //btc
            ),
            Hash(
                contract_id_from_str(ContractAddress::get_eth(args.network)).unwrap(), //eth
            ),
        ],
        backstop: Hash(
            contract_id_from_str(ContractAddress::get_backstop(args.network)).unwrap(), //Backstop address
        ),
        backstop_token_address: Hash(
            contract_id_from_str(ContractAddress::get_backstop_token(args.network)).unwrap(), //Backstop token address
        ),
        usdc_token_address: Hash(
            contract_id_from_str(ContractAddress::get_usdc(args.network)).unwrap(), //USDC token address
        ),
        bid_percentage: 10000000,
        oracle_id: Hash(
            contract_id_from_str(ContractAddress::get_oracle(args.network)).unwrap(), //oracle
        ),
        us: args.private_key.to_string(), //TODO: grab from args
        min_hf: 12000000,
        required_profit: 10000000,
        network_passphrase: passphrase.clone(),
        all_user_path: env::current_dir()?
            .join("all_users.csv")
            .to_str()
            .unwrap()
            .to_string(), //PATH for csv of all users
        oracle_decimals: 7,
    };
    let strategy = BlendAuctioneer::new(&config).await?;
    engine.add_strategy(Box::new(strategy));
    let strategy = BlendLiquidator::new(&config).await?;
    engine.add_strategy(Box::new(strategy));

    // Set up flashbots executor.
    // let executor = Box::new(MempoolExecutor::new(provider.clone()));
    // let executor = ExecutorMap::new(executor, |action| match action {
    //     Action::SubmitTx(tx) => Some(tx),
    // });
    // engine.add_executor(Box::new(executor));

    // Set up soroban executor.
    let executor =
        Box::new(SorobanExecutor::new(&config.rpc_url, &config.network_passphrase.clone()).await);
    let executor = ExecutorMap::new(executor, |action| match action {
        Action::SubmitTx(tx) => Some(tx),
    });
    engine.add_executor(Box::new(executor));

    // Start engine.
    if let Ok(mut set) = engine.run().await {
        while let Some(res) = set.join_next().await {
            info!("res: {:?}", res);
        }
    }
    Ok(())
}
