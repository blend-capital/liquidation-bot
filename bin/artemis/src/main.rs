use anyhow::Result;
use artemis_core::collectors::block_collector::BlockCollector;
use artemis_core::collectors::log_collector::{EventFilter, LogCollector};
use artemis_core::executors::soroban_executor::SorobanExecutor;
use blend_liquidator::strategy::BlendLiquidator;
use blend_liquidator::types::Config;
use clap::Parser;

// use artemis_core::collectors::block_collector::BlockCollector;
// use artemis_core::executors::soroban_executor::SorobanExecutor;
use blend_liquidator::types::{Action, Event};
use soroban_cli::rpc::EventType;
use stellar_xdr::curr::Hash;
// use opensea_sudo_arb::strategy::OpenseaSudoArb;
use tracing::{info, Level};
use tracing_subscriber::{filter, prelude::*};

use artemis_core::engine::Engine;
use artemis_core::types::{CollectorMap, ExecutorMap};
use soroban_cli::utils::contract_id_from_str;

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
    let path = "/usr/local/bin/stellar-core".to_string(); //TODO we need this for a core engine at some point

    // Set up engine.
    let mut engine: Engine<Event, Action> = Engine::default();

    // Set up log collector
    let log_collector = Box::new(LogCollector::new(
        "https://soroban-testnet.stellar.org".to_string(),
        EventFilter {
            event_type: EventType::Contract,
            contract_ids: vec![
                "CABFXDAA5BILXZPD7Y25QQPO77M5JHCCC3HWUBDGU52HNUHL4Z6YCTAC".to_string(), //stellar pool
                "CA2NWEPNC6BD5KELGJDVWWTXUE7ASDKTNQNL6DN3TGBVWFEWSVVGMUAF".to_string(), //oracle
            ],
            topics: vec![],
        },
    ));
    let log_collector = CollectorMap::new(log_collector, |e| Event::SorobanEvents(Box::new(e)));
    engine.add_collector(Box::new(log_collector));

    // Set up block collector.
    let block_collector = Box::new(BlockCollector::new(
        "https://soroban-testnet.stellar.org".to_string(),
    ));

    let block_collector = CollectorMap::new(block_collector, |e| Event::NewBlock(Box::new(e)));
    engine.add_collector(Box::new(block_collector));

    // Set up Blend Liquidator.
    let config = Config {
        rpc_url: "https://soroban-testnet.stellar.org".to_string(),
        pools: vec![Hash(
            contract_id_from_str("CABFXDAA5BILXZPD7Y25QQPO77M5JHCCC3HWUBDGU52HNUHL4Z6YCTAC")
                .unwrap(), //Stellar pool
        )],
        assets: vec![
            Hash(
                contract_id_from_str("CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU")
                    .unwrap(),
            ), //USDC
            Hash(
                contract_id_from_str("CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC")
                    .unwrap(), //XLM
            ),
        ],
        backstop: Hash(
            contract_id_from_str("CATLJNAQQIRHTAXCRPC3IHIUIQ6KQXNUUVCVGAOL2T2RPRPCNFT6MM4M")
                .unwrap(), //Backstop address
        ),
        backstop_token_address: Hash(
            contract_id_from_str("CAAQXUJO72JUATK75H7ZZHUBPXP3ECLT6Z5UOCTXGC6B4HKEP6VDPJ7S")
                .unwrap(), //Comet address - TODO: switch to backstop token
        ),
        bid_percentage: 10000000,
        oracle_id: Hash(
            contract_id_from_str("CA2NWEPNC6BD5KELGJDVWWTXUE7ASDKTNQNL6DN3TGBVWFEWSVVGMUAF")
                .unwrap(),
        ),
        us: args.private_key.to_string(), //TODO: grab from args
        min_hf: 12000000,
        required_profit: 10000000,
        network_passphrase: "Test SDF Network ; September 2015".to_string(),
    };
    let strategy = BlendLiquidator::new(&config).await;
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
