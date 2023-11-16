use ingest::{CaptiveCore, IngestionConfig};
use stellar_xdr::next::{ContractEvent, ContractEventType, LedgerCloseMeta, TransactionMeta, VecM};

const TARGET_SEQ: u32 = 387468;

pub fn main() {
    let config = IngestionConfig {
        executable_path: "/usr/local/bin/stellar-core".to_string(),
        context_path: Default::default(),
        network: ingest::SupportedNetwork::Futurenet,
        bounded_buffer_size: None,
        staggered: None,
    };

    let mut captive_core = CaptiveCore::new(config);

    // ...
    let receiver = captive_core.start_online_no_range().unwrap();
    println!(
        "Capturing all events. When a contract event will be emitted it will be printed to stdout"
    );
    for result in receiver.iter() {
        let ledger = result.ledger_close_meta.unwrap().ledger_close_meta;
        match &ledger {
            LedgerCloseMeta::V1(v1) => {
                let ledger_seq = v1.ledger_header.header.ledger_seq;
                if ledger_seq == TARGET_SEQ {
                    println!("Reached target ledger, closing");
                    captive_core.close_runner_process().unwrap();

                    std::process::exit(0)
                }

                for tx_processing in v1.tx_processing.iter() {
                    match &tx_processing.tx_apply_processing {
                        TransactionMeta::V3(meta) => {
                            if let Some(soroban) = &meta.soroban_meta {
                                if !soroban.events.is_empty() {
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
}
