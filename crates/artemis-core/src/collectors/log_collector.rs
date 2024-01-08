use core::time;
use std::thread::sleep;

use crate::types::{ Collector, CollectorStream };
use anyhow::Result;
use async_trait::async_trait;
use stellar_xdr::next::{ ContractEventType, LedgerCloseMeta, TransactionMeta, VecM };
use tokio::sync::broadcast;
use tokio_stream::{ wrappers::BroadcastStream, StreamExt };
use crossbeam_channel::{ Receiver };
use soroban::server::{ Server, GetEventRequest, EventFilter, ContractEvent, PaginationFilter };
/// A collector that listens for new blockchain event logs based on a [Filter](Filter),
/// and generates a stream of [events](Log).
pub struct LogCollector {
    client: Server,
    filters: Vec<EventFilter>,
    last_event_timestamp: u32,
    last_curser_id: Option<String>,
}

impl LogCollector {
    pub fn new(url: String, filters: Vec<EventFilter>) -> Self {
        Self { client: Server::new(&url), filters, last_event_timestamp: 0, last_curser_id: None }
    }
}

/// Implementation of the [Collector](Collector) trait for the [LogCollector](LogCollector).
#[async_trait]
impl Collector<ContractEvent> for LogCollector {
    async fn get_event_stream(&mut self) -> Result<CollectorStream<'_, ContractEvent>> {
        let (sender, receiver) = broadcast::channel(500000);
        let client = self.client.clone();
        let mut last_event_timestamp = self.last_event_timestamp;
        let mut last_cursor_id = self.last_curser_id.clone();
        let filters = self.filters.clone();
        tokio::spawn(async move {
            if last_event_timestamp == 0 {
                last_event_timestamp = client.get_latest_ledger().await.unwrap().sequence;
            }
            loop {
                let result: soroban::server::GetEventsResponse;
                if let Some(cursor_id) = last_cursor_id.clone() {
                    result = client
                        .get_events(GetEventRequest {
                            start_ledger: None,
                            filters: filters.clone(),
                            pagination: Some(PaginationFilter {
                                limit: None,
                                cursor: Some(cursor_id),
                            }),
                        }).await
                        .unwrap();
                    // println!("{:#?}", result);
                } else {
                    result = client
                        .get_events(GetEventRequest {
                            start_ledger: Some(last_event_timestamp),
                            filters: filters.clone(),
                            pagination: None,
                        }).await
                        .unwrap();
                    // println!("{:#?}", result);
                }

                if result.events.len() > 0 {
                    for event in result.events.into_iter() {
                        let _ = sender.send(event.clone());
                        last_cursor_id = Some(event.paging_token);
                        last_event_timestamp = event.ledger;
                    }
                }
                sleep(time::Duration::from_secs(1));
            }
            // while let Ok(result) = rx.recv() {
            //     let ledger = result.ledger_close_meta.unwrap().ledger_close_meta;

            //     match &ledger {
            //         LedgerCloseMeta::V2(v2) => {
            //             for tx_processing in v2.tx_processing.iter() {
            //                 match &tx_processing.tx_apply_processing {
            //                     TransactionMeta::V3(meta) => {
            //                         if let Some(soroban) = &meta.soroban_meta {
            //                             if !soroban.events.is_empty() {
            //                                 if sender.send(soroban.events.clone()).is_err() {
            //                                     break;
            //                                 }
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
        });
        let stream = BroadcastStream::new(receiver);
        let stream = stream.filter_map(|event| {
            if event.clone().unwrap().event_type == "contract" {
                Some(event.unwrap())
            } else {
                None
            }
        });
        println!("ABOUT TO RETURN");
        Ok(Box::pin(stream))
    }
}
