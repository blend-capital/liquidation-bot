use anyhow::Result;
use artemis_core::{
    collectors::{
        block_collector::BlockCollector,
        log_collector::{EventFilter, LogCollector},
    },
    engine::Engine,
    executors::soroban_executor::SorobanExecutor,
    types::{CollectorMap, ExecutorMap},
};
use blend_auctioneer::strategy::BlendAuctioneer;
use blend_liquidator::strategy::BlendLiquidator;
use blend_utilities::types::{Action, Config, Event};
use clap::Parser;
use ed25519_dalek::SigningKey;
use soroban_rpc::EventType;
use stellar_strkey::ed25519::PrivateKey;
use stellar_xdr::curr::ScAddress;

use serde_json;
use std::fs;
use tracing::{info, Level};
use tracing_subscriber::{filter, prelude::*};
/// CLI Options.
#[derive(Parser, Debug)]
pub struct Args {
    /// Private key for sending txs.
    #[arg(long)]
    pub private_key: String,
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

    let args = Args::parse();
    let config_data =
        fs::read_to_string("/opt/liquidation-bot/config.json").expect("Unable to read config file");
    let config: Config = serde_json::from_str(&config_data).expect("Unable to parse json");
    let signing_key =
        SigningKey::from_bytes(&PrivateKey::from_string(&args.private_key).unwrap().0);

    // Set up engine.
    let mut engine: Engine<Event, Action> = Engine::default();

    // Set up log collector
    let mut event_contract_ids = Vec::new();
    for contract in config.pools.iter() {
        event_contract_ids.push(ScAddress::Contract(contract.clone()).to_string());
    }
    event_contract_ids.push(ScAddress::Contract(config.oracle_id.clone()).to_string());
    let log_collector = Box::new(LogCollector::new(
        config.rpc_url.clone(),
        EventFilter {
            event_type: EventType::Contract,
            contract_ids: event_contract_ids,
            topics: vec![],
        },
    ));
    let log_collector = CollectorMap::new(log_collector, |e| Event::SorobanEvents(Box::new(e)));
    engine.add_collector(Box::new(log_collector));

    // Set up block collector.
    let block_collector = Box::new(BlockCollector::new(config.rpc_url.clone()));
    let block_collector = CollectorMap::new(block_collector, |e| Event::NewBlock(Box::new(e)));
    engine.add_collector(Box::new(block_collector));

    // Set up strategies.
    let strategy = BlendAuctioneer::new(&config, &signing_key).await?;
    engine.add_strategy(Box::new(strategy));
    let strategy = BlendLiquidator::new(&config, &signing_key).await?;
    engine.add_strategy(Box::new(strategy));

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
