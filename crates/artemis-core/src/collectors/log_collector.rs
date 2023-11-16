use crate::types::{Collector, CollectorStream};
use anyhow::Result;
use async_trait::async_trait;
use ingest::{
    BoundedRange, CaptiveCore, IngestionConfig, LedgerCloseMetaReader, LedgerCloseMetaWrapper,
    MetaResult, Range, SupportedNetwork,
};
use stellar_xdr::next::{ContractEvent, ContractEventType, LedgerCloseMeta, TransactionMeta, VecM};
use tokio::sync::broadcast::{self};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

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

        match core_receiver {
            Ok(result) => {
                // Process the result if it is not an error
                let (sender, mut receiver) = broadcast::channel(500000);

                std::thread::spawn(move || {
                    while let Ok(result) = result.recv() {
                        let ledger = result.ledger_close_meta.unwrap().ledger_close_meta;
                        match &ledger {
                            LedgerCloseMeta::V1(v1) => {
                                let ledger_seq = v1.ledger_header.header.ledger_seq;
                                // if ledger_seq == TARGET_SEQ {
                                //     println!("Reached target ledger, closing");
                                //     captive_core.close_runner_process().unwrap();

                                //     std::process::exit(0)
                                // }

                                for tx_processing in v1.tx_processing.iter() {
                                    match &tx_processing.tx_apply_processing {
                                        TransactionMeta::V3(meta) => {
                                            if let Some(soroban) = &meta.soroban_meta {
                                                if !soroban.events.is_empty() {
                                                    if sender.send(soroban.events.clone()).is_err()
                                                    {
                                                        break;
                                                    }

                                                    println!(
                                                        "Events for ledger {}: \n{:?}\n",
                                                        ledger_seq, soroban.events
                                                    )
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
                            Some(events.clone());
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

        // Ok(Box::pin(stream));
        // println!("Capturing all events. When a contract event will be emitted it will be printed to stdout");
        // for result in receiver.iter() {
        //     let ledger = result.ledger_close_meta.unwrap().ledger_close_meta;
        //     match &ledger {
        //         LedgerCloseMeta::V1(v1) => {
        //             let ledger_seq = v1.ledger_header.header.ledger_seq;
        //             // if ledger_seq == TARGET_SEQ {
        //             //     println!("Reached target ledger, closing");
        //             //     captive_core.close_runner_process().unwrap();

        //             //     std::process::exit(0)
        //             // }

        //             for tx_processing in v1.tx_processing.iter() {
        //                 match &tx_processing.tx_apply_processing {
        //                     TransactionMeta::V3(meta) => {
        //                         if let Some(soroban) = &meta.soroban_meta {
        //                             if !soroban.events.is_empty() {
        //                                 let events = soroban.events;
        //                                 Ok(Box::pin(soroban.events));

        //                                 // println!(
        //                                 //     "Events for ledger {}: \n{}\n",
        //                                 //     ledger_seq,
        //                                 //     serde_json::to_string_pretty(&soroban.events).unwrap()
        //                                 // )
        //                             }
        //                         }
        //                     }
        //                     _ => todo!(),
        //                 }
        //             }
        //         }
        //         _ => (),
        //     }
        // }

        // let stream = self.provider.subscribe_logs(&self.filter).await?;
        // let stream = stream.filter_map(Some);
        // Ok(Box::pin(stream))
    }
}
