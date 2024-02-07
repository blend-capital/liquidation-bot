use core::time;
use std::thread::sleep;

use crate::types::{Collector, CollectorStream};
use anyhow::Result;
use async_trait::async_trait;
use soroban_cli::rpc::{Client, Event, EventStart, EventType, GetEventsResponse};
use tokio::sync::broadcast;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
#[derive(Debug, Clone)]
pub struct EventFilter {
    pub event_type: EventType,
    pub contract_ids: Vec<String>,
    pub topics: Vec<String>,
}
/// A collector that listens for new blockchain event logs based on a [Filter](Filter),
/// and generates a stream of [events](Log).
pub struct LogCollector {
    network_url: String,
    filter: EventFilter,
    last_event_timestamp: u32,
    last_curser_id: Option<String>,
}

impl LogCollector {
    pub fn new(url: String, filter: EventFilter) -> Self {
        Self {
            network_url: url,
            filter,
            last_event_timestamp: 0,
            last_curser_id: None,
        }
    }
}

/// Implementation of the [Collector](Collector) trait for the [LogCollector](LogCollector).
#[async_trait]
impl Collector<Event> for LogCollector {
    async fn get_event_stream(&mut self) -> Result<CollectorStream<'_, Event>> {
        let (sender, receiver) = broadcast::channel(500000);
        let mut last_event_timestamp = self.last_event_timestamp;
        let mut last_cursor_id = self.last_curser_id.clone();
        let filter = self.filter.clone();
        let network_url = self.network_url.clone();
        tokio::spawn(async move {
            let client = Client::new(&network_url).unwrap();
            if last_event_timestamp == 0 {
                last_event_timestamp = client.get_latest_ledger().await.unwrap().sequence;
            }
            loop {
                let result: GetEventsResponse;
                if let Some(cursor_id) = last_cursor_id.clone() {
                    result = client
                        .get_events(
                            EventStart::Cursor(cursor_id),
                            Some(EventType::Contract),
                            filter.contract_ids.as_slice(),
                            filter.topics.as_slice(),
                            None,
                        )
                        .await
                        .unwrap();
                } else {
                    result = client
                        .get_events(
                            EventStart::Ledger(last_event_timestamp),
                            Some(EventType::Contract),
                            filter.contract_ids.as_slice(),
                            filter.topics.as_slice(),
                            None,
                        )
                        .await
                        .unwrap();
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
        });
        let stream = BroadcastStream::new(receiver);
        let stream = stream.filter_map(|event| {
            if event.clone().unwrap().event_type == "contract" {
                Some(event.unwrap())
            } else {
                None
            }
        });
        Ok(Box::pin(stream))
    }
}
