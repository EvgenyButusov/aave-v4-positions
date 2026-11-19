use std::collections::HashMap;

use alloy_primitives::{Address, B256, Bytes, address, b256};
use indexing::{ChainError, LogFilter, LogReader as _, RawLog};
use serde_json::{Value, json};

use crate::harness::{harness, over_one};
use crate::json_rpc::{Log, Unexpected};
use crate::scripted::{Reply, ScriptedNetwork, hash, header, hex};

const SPOKE: Address = address!("0x94e7a5dcbe816e498b89ab752661904e2f56c485");
const HUB: Address = address!("0xcca852bc40e560adc3b1cc58ca5b55638ce826c9");
const SUPPLY: B256 = b256!("0x1111111111111111111111111111111111111111111111111111111111111111");
const BORROW: B256 = b256!("0x2222222222222222222222222222222222222222222222222222222222222222");

/// Blocks 100 to 200 of the Spoke and the Hub.
fn filter(topic0: Vec<B256>) -> LogFilter {
    LogFilter {
        addresses: vec![SPOKE, HUB],
        topic0,
        from_block: 100,
        to_block: 200,
    }
}

/// A log as a node serves it, at `block` and with `timestamp` when the node includes one.
fn log(block: u64, index: u64, timestamp: Option<u64>) -> Value {
    let mut log = json!({
        "address": format!("{SPOKE:#x}"),
        "topics": [format!("{SUPPLY:#x}")],
        "data": "0xdeadbeef",
        "blockNumber": hex(block),
        "blockHash": hash('b'),
        "transactionHash": hash('c'),
        "transactionIndex": hex(3),
        "logIndex": hex(index),
        "removed": false,
    });
    if let Some(timestamp) = timestamp {
        log["blockTimestamp"] = hex(timestamp);
    }
    log
}

fn refusal(message: &'static str) -> Reply {
    Reply::Error {
        code: -32602,
        message,
    }
}

#[tokio::test]
async fn sends_the_addresses_range_and_topic_filter_to_the_provider() {
    let (network, harness) = over_one(|_, _| Reply::Result(json!([]))).await;

    harness
        .rpc
        .logs(&filter(vec![SUPPLY, BORROW]))
        .await
        .unwrap();

    let params = &network.calls()[0].params[0];
    let mut addresses: Vec<String> = serde_json::from_value(params["address"].clone()).unwrap();
    addresses.sort();
    let mut topic0: Vec<String> = serde_json::from_value(params["topics"][0].clone()).unwrap();
    topic0.sort();

    assert_eq!(params["fromBlock"], "0x64");
    assert_eq!(params["toBlock"], "0xc8");
    assert_eq!(addresses, [format!("{SPOKE:#x}"), format!("{HUB:#x}")]);
    assert_eq!(topic0, [format!("{SUPPLY:#x}"), format!("{BORROW:#x}")]);
}

#[tokio::test]
async fn omits_the_topic_filter_entirely_when_no_signature_is_given() {
    let (network, harness) = over_one(|_, _| Reply::Result(json!([]))).await;

    harness.rpc.logs(&filter(vec![])).await.unwrap();

    let params = &network.calls()[0].params[0];
    assert!(params.get("topics").is_none(), "{params}");
}

#[tokio::test]
async fn converts_every_quantity_in_a_log() {
    let (_, harness) =
        over_one(|_, _| Reply::Result(json!([log(150, 7, Some(1_700_000_000))]))).await;

    let logs = harness.rpc.logs(&filter(vec![SUPPLY])).await.unwrap();

    assert_eq!(
        logs,
        [RawLog {
            address: SPOKE,
            topics: vec![SUPPLY],
            data: Bytes::from_static(&[0xde, 0xad, 0xbe, 0xef]),
            block_number: 150,
            block_hash: hash('b').parse().unwrap(),
            block_timestamp: 1_700_000_000,
            transaction_hash: hash('c').parse().unwrap(),
            transaction_index: 3,
            log_index: 7,
        }]
    );
}

#[tokio::test]
async fn uses_the_providers_timestamp_without_a_single_extra_request() {
    let (network, harness) =
        over_one(|_, _| Reply::Result(json!([log(150, 0, Some(1)), log(151, 0, Some(2))]))).await;

    harness.rpc.logs(&filter(vec![])).await.unwrap();

    assert_eq!(network.asked(), [("primary".into(), "eth_getLogs".into())]);
}

#[tokio::test]
async fn reads_one_header_per_distinct_block_when_the_provider_omits_the_timestamp() {
    let (network, harness) = over_one(|method, params| match method {
        "eth_getLogs" => Reply::Result(json!([
            log(150, 0, None),
            log(150, 1, None),
            log(151, 0, None)
        ])),
        _ => {
            let number =
                u64::from_str_radix(params[0].as_str().unwrap().trim_start_matches("0x"), 16)
                    .unwrap();
            Reply::Result(header(number, number * 10))
        }
    })
    .await;

    let logs = harness.rpc.logs(&filter(vec![])).await.unwrap();

    let stamped: Vec<(u64, u64)> = logs
        .iter()
        .map(|log| (log.block_number, log.block_timestamp))
        .collect();
    assert_eq!(stamped, [(150, 1500), (150, 1500), (151, 1510)]);
    let headers = network
        .asked()
        .iter()
        .filter(|(_, method)| method == "eth_getBlockByNumber")
        .count();
    assert_eq!(headers, 2);
}

#[test]
fn refuses_a_log_missing_any_one_field_that_places_it_in_a_block() {
    for field in [
        "blockNumber",
        "blockHash",
        "transactionHash",
        "transactionIndex",
        "logIndex",
    ] {
        let mut pending = log(150, 0, Some(1));
        pending[field] = Value::Null;
        let log: Log = serde_json::from_value(pending).unwrap();

        let refusal = log.into_raw(&HashMap::new()).unwrap_err();

        assert!(matches!(refusal, Unexpected::Pending), "{field}: {refusal}");
    }
}

#[tokio::test]
async fn names_a_range_refusal_so_the_caller_can_narrow() {
    let detail = "Block range too large: maximum allowed is 50 blocks";
    let (_, harness) = over_one(move |_, _| refusal(detail)).await;

    let refused = harness.rpc.logs(&filter(vec![])).await.unwrap_err();

    match refused {
        ChainError::RangeTooLarge {
            from,
            to,
            detail: named,
        } => {
            assert_eq!((from, to, named.as_str()), (100, 200, detail));
        }
        other => panic!("expected a range refusal, got {other}"),
    }
}

#[tokio::test]
async fn takes_the_last_providers_refusal_when_every_one_refuses() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", |_, _| {
            refusal("Archive requests require a personal token")
        })
        .await;
    let secondary = network
        .node("secondary", |_, _| {
            refusal("eth_getLogs is limited to 0 - 50 blocks range")
        })
        .await;

    let refused = harness(&[&primary.url, &secondary.url])
        .rpc
        .logs(&filter(vec![]))
        .await
        .unwrap_err();

    assert!(
        matches!(refused, ChainError::RangeTooLarge { .. }),
        "{refused}"
    );
}

#[tokio::test]
async fn an_ordinary_failure_is_unavailable_and_not_a_range_refusal() {
    let (_, harness) = over_one(|_, _| Reply::Status(500)).await;

    let refused = harness.rpc.logs(&filter(vec![])).await.unwrap_err();

    assert!(
        matches!(refused, ChainError::Unavailable { .. }),
        "{refused}"
    );
}
