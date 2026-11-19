use std::collections::HashMap;

use alloy_primitives::{Address, B256, Bytes, address, b256};
use indexing::{BlockHeader, ChainClient as _, ChainError, LogFilter, LogReader as _, RawLog};
use serde_json::{Value, json};
use units::ChainId;

use super::{Log, Unexpected};
use crate::harness::harness;
use crate::scripted::{Reply, ScriptedNetwork, hex};

const SPOKE: Address = address!("0x94e7a5dcbe816e498b89ab752661904e2f56c485");
const HUB: Address = address!("0xcca852bc40e560adc3b1cc58ca5b55638ce826c9");
const SUPPLY: B256 = b256!("0x1111111111111111111111111111111111111111111111111111111111111111");
const BORROW: B256 = b256!("0x2222222222222222222222222222222222222222222222222222222222222222");

fn filter(topic0: Vec<B256>) -> LogFilter {
    LogFilter {
        addresses: vec![SPOKE, HUB],
        topic0,
        from_block: 100,
        to_block: 200,
    }
}

fn hash(byte: char) -> String {
    format!("0x{}", byte.to_string().repeat(64))
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

fn header(number: u64, timestamp: u64) -> Value {
    json!({
        "number": hex(number),
        "hash": hash('a'),
        "parentHash": hash('b'),
        "timestamp": hex(timestamp),
        "transactions": [],
    })
}

#[tokio::test]
async fn builds_its_client_without_reaching_the_network() {
    let network = ScriptedNetwork::new();
    let primary = network.node("primary", |_, _| Reply::Result(hex(1))).await;

    let _client = harness(&[&primary.url]);

    assert!(network.asked().is_empty());
}

#[tokio::test]
async fn reads_the_chain_id() {
    let network = ScriptedNetwork::new();
    let primary = network.node("primary", |_, _| Reply::Result(hex(1))).await;

    let chain_id = harness(&[&primary.url]).rpc.chain_id().await.unwrap();

    assert_eq!(chain_id, ChainId::known(1));
}

#[tokio::test]
async fn refuses_a_chain_id_no_chain_can_have() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", |_, _| Reply::Result(hex(1 << 53)))
        .await;

    let refusal = harness(&[&primary.url]).rpc.chain_id().await.unwrap_err();

    assert!(
        matches!(refusal, ChainError::Unavailable { .. }),
        "{refusal}"
    );
}

#[tokio::test]
async fn reads_the_head_as_a_plain_height() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", |_, _| Reply::Result(json!("0x1793603")))
        .await;

    let head = harness(&[&primary.url])
        .rpc
        .head_block_number()
        .await
        .unwrap();

    assert_eq!(head, 24_720_899);
}

#[tokio::test]
async fn re_reads_the_head_on_every_call_rather_than_serving_a_cached_one() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node(
            "primary",
            crate::scripted::in_turn(|nth| Reply::Result(hex(100 + nth as u64))),
        )
        .await;

    let harness = harness(&[&primary.url]);

    assert_eq!(harness.rpc.head_block_number().await.unwrap(), 100);
    assert_eq!(harness.rpc.head_block_number().await.unwrap(), 101);
}

#[tokio::test]
async fn reads_a_header_from_the_four_fields_it_keeps() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", |_, _| Reply::Result(header(100, 0x6600)))
        .await;

    let header = harness(&[&primary.url])
        .rpc
        .block_header(100)
        .await
        .unwrap();

    assert_eq!(
        header,
        BlockHeader {
            number: 100,
            hash: hash('a').parse().unwrap(),
            parent_hash: hash('b').parse().unwrap(),
            timestamp: 0x6600,
        }
    );
    assert_eq!(
        network.calls()[0].params,
        json!(["0x64", false]),
        "asks for the header without its transactions"
    );
}

#[tokio::test]
async fn a_block_the_node_does_not_have_is_an_error_and_not_a_default() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", |_, _| Reply::Result(Value::Null))
        .await;

    let refusal = harness(&[&primary.url])
        .rpc
        .block_header(100)
        .await
        .unwrap_err();

    assert!(
        refusal.to_string().contains("could not be read"),
        "{refusal}"
    );
}

#[tokio::test]
async fn sends_the_addresses_range_and_topic_filter_to_the_provider() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", |_, _| Reply::Result(json!([])))
        .await;

    harness(&[&primary.url])
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
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", |_, _| Reply::Result(json!([])))
        .await;

    harness(&[&primary.url])
        .rpc
        .logs(&filter(vec![]))
        .await
        .unwrap();

    let params = &network.calls()[0].params[0];
    assert!(params.get("topics").is_none(), "{params}");
}

#[tokio::test]
async fn converts_every_quantity_in_a_log() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", |_, _| {
            Reply::Result(json!([log(150, 7, Some(1_700_000_000))]))
        })
        .await;

    let logs = harness(&[&primary.url])
        .rpc
        .logs(&filter(vec![SUPPLY]))
        .await
        .unwrap();

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
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", |_, _| {
            Reply::Result(json!([log(150, 0, Some(1)), log(151, 0, Some(2))]))
        })
        .await;

    harness(&[&primary.url])
        .rpc
        .logs(&filter(vec![]))
        .await
        .unwrap();

    assert_eq!(network.asked(), [("primary".into(), "eth_getLogs".into())]);
}

#[tokio::test]
async fn reads_one_header_per_distinct_block_when_the_provider_omits_the_timestamp() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", |method, params| match method {
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

    let logs = harness(&[&primary.url])
        .rpc
        .logs(&filter(vec![]))
        .await
        .unwrap();

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

#[tokio::test]
async fn refuses_a_log_that_belongs_to_no_block() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", |_, _| {
            let mut pending = log(150, 0, Some(1));
            pending["blockNumber"] = Value::Null;
            pending["blockHash"] = Value::Null;
            Reply::Result(json!([pending]))
        })
        .await;

    let refusal = harness(&[&primary.url])
        .rpc
        .logs(&filter(vec![]))
        .await
        .unwrap_err();

    assert!(
        matches!(refusal, ChainError::Unavailable { .. }),
        "{refusal}"
    );
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

#[test]
fn refuses_a_log_whose_timestamp_neither_it_nor_a_header_gave() {
    let log: Log = serde_json::from_value(log(150, 0, None)).unwrap();

    let refusal = log.into_raw(&HashMap::from([(151, 1510)])).unwrap_err();

    assert!(matches!(refusal, Unexpected::NoTimestamp(150)), "{refusal}");
}

#[tokio::test]
async fn names_a_range_refusal_so_the_caller_can_narrow() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", |_, _| Reply::Error {
            code: -32602,
            message: "Block range too large: maximum allowed is 50 blocks",
        })
        .await;

    let refusal = harness(&[&primary.url])
        .rpc
        .logs(&filter(vec![]))
        .await
        .unwrap_err();

    match refusal {
        ChainError::RangeTooLarge { from, to, detail } => {
            assert_eq!((from, to), (100, 200));
            assert_eq!(
                detail,
                "Block range too large: maximum allowed is 50 blocks"
            );
        }
        other => panic!("expected a range refusal, got {other}"),
    }
}

#[tokio::test]
async fn takes_the_last_providers_refusal_when_every_one_refuses() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", |_, _| Reply::Error {
            code: -32602,
            message: "Archive requests require a personal token",
        })
        .await;
    let secondary = network
        .node("secondary", |_, _| Reply::Error {
            code: -32602,
            message: "eth_getLogs is limited to 0 - 50 blocks range",
        })
        .await;

    let refusal = harness(&[&primary.url, &secondary.url])
        .rpc
        .logs(&filter(vec![]))
        .await
        .unwrap_err();

    assert!(
        matches!(refusal, ChainError::RangeTooLarge { .. }),
        "{refusal}"
    );
}

#[tokio::test]
async fn an_ordinary_failure_is_unavailable_and_not_a_range_refusal() {
    let network = ScriptedNetwork::new();
    let primary = network.node("primary", |_, _| Reply::Status(500)).await;

    let refusal = harness(&[&primary.url])
        .rpc
        .logs(&filter(vec![]))
        .await
        .unwrap_err();

    assert!(
        matches!(refusal, ChainError::Unavailable { .. }),
        "{refusal}"
    );
}
