use anyhow::Result;
use artemis_core::collectors::block_collector::BlockCollector;
use artemis_core::collectors::log_collector::{ LogCollector, EventFilter };
use blend_liquidator::strategy::BlendLiquidator;
use blend_liquidator::types::Config;
use clap::Parser;

// use artemis_core::collectors::block_collector::BlockCollector;
// use artemis_core::executors::soroban_executor::SorobanExecutor;
use blend_liquidator::types::{ Action, Event };
use soroban_cli::rpc::EventType;
use stellar_xdr::curr::Hash;
// use opensea_sudo_arb::strategy::OpenseaSudoArb;
use tracing::{ info, Level };
use tracing_subscriber::{ filter, prelude::* };

use artemis_core::engine::Engine;
use artemis_core::types::{ CollectorMap, ExecutorMap };
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
    let filter = filter::Targets
        ::new()
        .with_target("opensea_sudo_arb", Level::INFO)
        .with_target("artemis_core", Level::INFO);
    tracing_subscriber::registry().with(tracing_subscriber::fmt::layer()).with(filter).init();

    let args = Args::parse();
    let path = "/usr/local/bin/stellar-core".to_string();

    // Set up engine.
    let mut engine: Engine<Event, Action> = Engine::default();

    // Set up log collector
    let log_collector = Box::new(
        LogCollector::new("https://rpc-futurenet.stellar.org".to_string(), EventFilter {
            event_type: EventType::Contract,
            contract_ids: vec![
                "CDQK7W4WSTNHIKQXJA2OVQP54J2JKZF62VODY3IBPTFI3UBP5HFAACTQ".to_string(),
                "CCJVZZ64S5B5ROM4V3D4V3GY77TAE6R2WAGD32PBFUEAJSQXFYQCLNA3".to_string()
            ],
            topics: vec![],
        })

    );
    let log_collector = CollectorMap::new(log_collector, |e| Event::SorobanEvents(Box::new(e)));
    engine.add_collector(Box::new(log_collector));

    // Set up block collector.
    let block_collector = Box::new(
        BlockCollector::new("https://rpc-futurenet.stellar.org".to_string())
    );

    let block_collector = CollectorMap::new(block_collector, |e| Event::NewBlock(Box::new(e)));
    engine.add_collector(Box::new(block_collector));

    // Set up opensea sudo arb strategy.
    let config = Config {
        rpc_url: "https://rpc-futurenet.stellar.org".to_string(),
        pools: vec![
            Hash(
                contract_id_from_str(
                    "CDQK7W4WSTNHIKQXJA2OVQP54J2JKZF62VODY3IBPTFI3UBP5HFAACTQ"
                ).unwrap()
            )
        ],
        assets: vec![
            Hash(
                contract_id_from_str(
                    "CB64D3G7SM2RTH6JSGG34DDTFTQ5CFDKVDZJZSODMCX4NJ2HV2KN7OHT"
                ).unwrap()
            ),
            Hash(
                contract_id_from_str(
                    "CCGVJJ3PCYXJGTB3BDNDB3WKTIJ3ITK3RPHIVC5O3GTK6H77QSGU7ZHQ"
                ).unwrap()
            )
        ],
        bid_percentage: 10000000,
        oracle_id: Hash(
            contract_id_from_str(
                "CAZV7LNSOHWU2WQYIIMF4BJTYREFI55PAXLVLT4YXSYXMIE3RM25AX2S"
            ).unwrap()
        ),
        us: "SBEOG3XOA5XVZBFUORLM5B5LAKGZLPPSUAYMWVM6VXQZW7NIA3VPHRKM".to_string(),
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
