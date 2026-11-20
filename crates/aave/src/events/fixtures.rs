//! Real mainnet logs, each beside the row the TypeScript indexer wrote for it: see `fixtures/golden.json`.

use std::sync::LazyLock;

use crate::MAIN_SPOKE_ON_ETHEREUM as FOLLOWED;
use alloy_primitives::Address;
use indexing::RawLog;
use serde_json::Value;

use crate::events::{Contract, DecodedEvent, Decoder};

static GOLDEN: LazyLock<Value> =
    LazyLock::new(|| serde_json::from_str(include_str!("../../fixtures/golden.json")).unwrap());

/// Each captured log of `contract`'s ledger, with the row written for it.
pub(crate) fn golden(contract: Contract) -> Vec<(RawLog, &'static Value)> {
    GOLDEN[contract.table().trim_end_matches("_events")]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| (raw_log(&entry["log"]), &entry["row"]))
        .collect()
}

pub(crate) fn decoder(contract: Contract) -> Decoder {
    Decoder::new(FOLLOWED.chain_id, contract, address(contract))
}

pub(crate) fn address(contract: Contract) -> Address {
    match contract {
        Contract::Spoke => FOLLOWED.spoke,
        Contract::Hub => FOLLOWED.hub,
    }
}

/// The captured log `event` of `contract`, decoded.
pub(crate) fn event(contract: Contract, event: &str) -> DecodedEvent {
    let (log, _) = golden(contract)
        .into_iter()
        .find(|(_, row)| row["event_name"] == event)
        .unwrap();
    decoder(contract).decode(&[log]).unwrap().remove(0)
}

pub(crate) fn raw_log(log: &Value) -> RawLog {
    let text = |key: &str| log[key].as_str().unwrap();
    let number = |key: &str| log[key].as_u64().unwrap();

    RawLog {
        address: text("address").parse().unwrap(),
        topics: log["topics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|topic| topic.as_str().unwrap().parse().unwrap())
            .collect(),
        data: text("data").parse().unwrap(),
        block_number: number("blockNumber"),
        block_hash: text("blockHash").parse().unwrap(),
        block_timestamp: number("blockTimestamp"),
        transaction_hash: text("transactionHash").parse().unwrap(),
        transaction_index: number("transactionIndex"),
        log_index: number("logIndex"),
    }
}
