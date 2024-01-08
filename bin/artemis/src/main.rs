use anyhow::Result;
use artemis_core::collectors::block_collector::BlockCollector;
use artemis_core::collectors::log_collector::LogCollector;
use blend_liquidator::strategy::BlendLiquidator;
use blend_liquidator::types::Config;
use clap::Parser;

// use artemis_core::collectors::block_collector::BlockCollector;
// use artemis_core::executors::soroban_executor::SorobanExecutor;
use blend_liquidator::types::{ Action, Event };
use stellar_xdr::next::Hash;
// use opensea_sudo_arb::strategy::OpenseaSudoArb;
use tracing::{ info, Level };
use tracing_subscriber::{ filter, prelude::* };

use std::str::FromStr;
use std::sync::{ Arc };
use std::sync::mpsc::{ channel };

use artemis_core::engine::Engine;
use artemis_core::types::{ CollectorMap, ExecutorMap };
use soroban::server::{ Server, EventFilter, EventType };
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
    let filter = filter::Targets
        ::new()
        .with_target("opensea_sudo_arb", Level::INFO)
        .with_target("artemis_core", Level::INFO);
    tracing_subscriber::registry().with(tracing_subscriber::fmt::layer()).with(filter).init();

    let args = Args::parse();
    let server = Server::new("https://soroban-testnet.stellar.org");
    // Set up ethers provider.
    // let ws = Ws::connect(args.wss).await?;
    // let provider = Provider::new(ws);

    // let wallet: LocalWallet = args.private_key.parse().unwrap();
    // let address = wallet.address();

    // let provider = Arc::new(provider.nonce_manager(address).with_signer(wallet));

    // Set up opensea client.
    // let opensea_client = OpenSeaV2Client::new(OpenSeaApiConfig {
    //     api_key: args.opensea_api_key.clone(),
    // });

    //set up stellar core ingestion config details
    // let network = match args.network {
    //     0 => SupportedNetwork::Futurenet,
    //     1 => SupportedNetwork::Pubnet,
    //     2 => SupportedNetwork::Testnet,
    //     _ => SupportedNetwork::Pubnet,
    // };
    let path = "/usr/local/bin/stellar-core".to_string();

    // Set up engine.
    let mut engine: Engine<Event, Action> = Engine::default();

    // Set up opensea collector.
    // let opensea_collector = Box::new(OpenseaOrderCollector::new(args.opensea_api_key));
    // let opensea_collector =
    //     CollectorMap::new(opensea_collector, |e| Event::OpenseaOrder(Box::new(e)));
    // engine.add_collector(Box::new(opensea_collector));

    // Set up log collector
    let log_collector = Box::new(
        LogCollector::new(
            "http://127.0.0.1:8000".to_string(),
            vec![EventFilter {
                event_type: EventType::Contract,
                contract_ids: Some(
                    vec![
                        "CB34BESMYNFFXXZHJTVX5MNPOR7N7PPI2JPABGMD4NYR6RWVWOG2FUYH".to_string(),
                        "CBTPEMBL2FPUNVREX6SY6SJ5PEZAXLQVQGDQWYJ6RBJBUZJG6YRTQCHH".to_string()
                    ]
                ),
                topics: None,
            }]
        )
    );
    let log_collector = CollectorMap::new(log_collector, |e| Event::SorobanEvents(Box::new(e)));
    engine.add_collector(Box::new(log_collector));

    // Set up block collector.
    let block_collector = Box::new(BlockCollector::new("http://127.0.0.1:8000".to_string()));
    let block_collector = CollectorMap::new(block_collector, |e| Event::NewBlock(Box::new(e)));
    engine.add_collector(Box::new(block_collector));

    // Set up opensea sudo arb strategy.
    let config = Config {
        pools: Vec::new(),
        assets: Vec::new(),
        bid_percentage: 10000000,
        oracle_id: Default::default(),
        us: Default::default(),
        min_hf: 10000000,
    };
    let strategy = BlendLiquidator::new(config);
    engine.add_strategy(Box::new(strategy));

    // Set up flashbots executor.
    // let executor = Box::new(MempoolExecutor::new(provider.clone()));
    // let executor = ExecutorMap::new(executor, |action| match action {
    //     Action::SubmitTx(tx) => Some(tx),
    // });
    // engine.add_executor(Box::new(executor));

    // Start engine.
    if let Ok(mut set) = engine.run().await {
        while let Some(res) = set.join_next().await {
            info!("res: {:?}", res);
        }
    }
    Ok(())
}
