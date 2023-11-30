use crate::types::{ Collector, CollectorStream };
use anyhow::Result;
use async_trait::async_trait;
use ingest::{ IngestionConfig, SupportedNetwork, CaptiveCore };
use stellar_xdr::next::{ ContractEvent, ContractEventType, LedgerCloseMeta, TransactionMeta, VecM };
use tokio::sync::broadcast::{ self };
use tokio_stream::{ wrappers::BroadcastStream, StreamExt };

/// A collector that listens for new blockchain event logs based on a [Filter](Filter),
/// and generates a stream of [events](Log).
pub struct LogCollector {
    network: SupportedNetwork,
    executable_path: String,
}

impl LogCollector {
    pub fn new(network: SupportedNetwork, executable_path: String) -> Self {
        Self {
            network,
            executable_path,
        }
    }
}

/// Implementation of the [Collector](Collector) trait for the [LogCollector](LogCollector).
/// This implementation uses the [PubsubClient](PubsubClient) to subscribe to new logs.
#[async_trait]
impl Collector<VecM<ContractEvent>> for LogCollector {
    async fn get_event_stream(&self) -> Result<CollectorStream<'_, VecM<ContractEvent>>> {
        let config = IngestionConfig {
            executable_path: self.executable_path.clone(),
            context_path: Default::default(),
            network: self.network,
            bounded_buffer_size: None,
            staggered: None,
        };
        println!("Creating captive core");
        let mut captive_core = CaptiveCore::new(config);
        let core_receiver = captive_core.start_online_no_range();
        let (sender, receiver) = broadcast::channel(500000);

        match core_receiver {
            Ok(result) => {
                // Process the result if it is not an error
                std::thread::spawn(move || {
                    while let Ok(result) = result.recv() {
                        let ledger = result.ledger_close_meta.unwrap().ledger_close_meta;
                        match &ledger {
                            LedgerCloseMeta::V2(v2) => {
                                for tx_processing in v2.tx_processing.iter() {
                                    match &tx_processing.tx_apply_processing {
                                        TransactionMeta::V3(meta) => {
                                            if let Some(soroban) = &meta.soroban_meta {
                                                if !soroban.events.is_empty() {
                                                    if sender.send(soroban.events.clone()).is_err() {
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                        _ => todo!(),
                                    }
                                }
                            }
                            _ => (),
                        }
                    }
                });

                let stream = BroadcastStream::new(receiver);
                let stream = stream.filter_map(|event| {
                    let events = event.unwrap();
                    for event in events.iter() {
                        if event.type_ == ContractEventType::Contract {
                            return Some(events.clone());
                        }
                    }
                    None
                });
                Ok(Box::pin(stream)) // don't specify this if I don't have to
            }
            Err(error) => {
                // Log the error if it is one
                eprintln!("Error: {:?}", error);
                panic!();
            }
        }
    }
}
