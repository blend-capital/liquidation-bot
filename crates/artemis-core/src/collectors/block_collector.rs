use crate::types::{ Collector, CollectorStream };
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::broadcast::{ self };
use tokio_stream::{ wrappers::BroadcastStream, StreamExt };
use core::time;
use std::thread::sleep;
use soroban_cli::rpc::Client;
// / A collector that listens for new blockchain event logs based on a [Filter](Filter),
/// and generates a stream of [events](Log).
pub struct BlockCollector {
    network_url: String,
    last_block_num: u32,
}

impl BlockCollector {
    pub fn new(url: String) -> Self {
        Self { network_url: url, last_block_num: 0 }
    }
}

/// A new block event, containing the block number and hash.
#[derive(Debug, Clone)]
pub struct NewBlock {
    pub number: u32,
}
#[async_trait]
impl Collector<NewBlock> for BlockCollector {
    async fn get_event_stream(&mut self) -> Result<CollectorStream<'_, NewBlock>> {
        let (sender, receiver) = broadcast::channel(500000);
        let mut last_block_num = self.last_block_num;
        let url = self.network_url.clone();
        tokio::spawn(async move {
            let server = Client::new(&url).unwrap();
            loop {
                let result = server.get_latest_ledger().await.unwrap();
                if result.sequence > last_block_num {
                    last_block_num = result.sequence;
                    let _ = sender.send(NewBlock { number: result.sequence });
                }
                sleep(time::Duration::from_secs(1));
            }
        });
        let stream = BroadcastStream::new(receiver);
        let stream = stream.filter_map(|block| { Some(block.unwrap()) });
        Ok(Box::pin(stream)) // don't specify this if I don't have to
    }
}
