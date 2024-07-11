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
use blend_strategies::{
    auctioneer_strategy::BlendAuctioneer,
    liquidation_strategy::BlendLiquidator,
    types::{Action, Config, Event},
};
use clap::Parser;
use ed25519_dalek::SigningKey;
use stellar_rpc_client::EventType;
use stellar_strkey::ed25519::PrivateKey;

use core::panic;
use serde_json;
use std::{
    fs::{self, OpenOptions},
    path::Path,
    sync::Arc,
};
use tracing::{info, Level};
use tracing_subscriber::{filter, prelude::*};
/// CLI Options.
#[derive(Parser, Debug)]
pub struct Args {
    #[arg(long)]
    pub config_path: String,
    /// Private key for sending txs.
    #[arg(long)]
    pub private_key: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config_data = fs::read_to_string(args.config_path).expect("Unable to read config file");
    let config: Config = serde_json::from_str(&config_data).expect("Unable to parse json");

    // Set up tracing and parse args.
    let filter = filter::Targets::new()
        .with_target("artemis_core", Level::INFO)
        .with_target("blend_strategies::auctioneer_strategy", Level::INFO)
        .with_target("blend_strategies::liquidation_strategy", Level::INFO)
        .with_target("blend_strategies::auction_manager", Level::INFO)
        .with_target("blend_strategies::db_manager", Level::INFO)
        .with_target("blend_strategies::helper", Level::INFO);

    let log_file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(Path::new(&config.db_path).join("logs.txt"));
    let log_file = match log_file {
        Ok(file) => file,
        Err(err) => panic!("Error: {:?}", err),
    };
    let log = tracing_subscriber::fmt::layer().with_writer(Arc::new(log_file));
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter.and_then(log))
        .init();

    let signing_key =
        SigningKey::from_bytes(&PrivateKey::from_string(&args.private_key).unwrap().0);

    // Set up engine.
    let mut engine: Engine<Event, Action> = Engine::default();

    // Set up log collector

    let log_collector = Box::new(LogCollector::new(
        config.rpc_url.clone(),
        EventFilter {
            event_type: EventType::Contract,
            contract_ids: config.pools.clone(),
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
    let executor = Box::new(
        SorobanExecutor::new(
            &config.rpc_url,
            &config.network_passphrase.clone(),
            &config.db_path,
            &config.slack_api_url_key,
        )
        .await,
    );
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
