use indexing::{BlockHeader, ChainClient as _, ChainError};
use serde_json::{Value, json};
use units::ChainId;

use crate::harness::over_one;
use crate::scripted::{Reply, hash, header, hex, in_turn};

#[tokio::test]
async fn builds_its_client_without_reaching_the_network() {
    let (network, _harness) = over_one(|_, _| Reply::Result(hex(1))).await;

    assert!(network.asked().is_empty());
}

#[tokio::test]
async fn reads_the_chain_id() {
    let (_, harness) = over_one(|_, _| Reply::Result(hex(1))).await;

    assert_eq!(harness.rpc.chain_id().await.unwrap(), ChainId::known(1));
}

#[tokio::test]
async fn refuses_a_chain_id_no_chain_can_have() {
    let (_, harness) = over_one(|_, _| Reply::Result(hex(1 << 53))).await;

    let refusal = harness.rpc.chain_id().await.unwrap_err();

    assert!(
        matches!(refusal, ChainError::Unavailable { .. }),
        "{refusal}"
    );
}

#[tokio::test]
async fn re_reads_the_head_on_every_call_rather_than_serving_a_cached_one() {
    let (_, harness) = over_one(in_turn(|nth| Reply::Result(hex(100 + nth as u64)))).await;

    assert_eq!(harness.rpc.head_block_number().await.unwrap(), 100);
    assert_eq!(harness.rpc.head_block_number().await.unwrap(), 101);
}

#[tokio::test]
async fn reads_a_header_from_the_four_fields_it_keeps() {
    let (network, harness) = over_one(|_, _| Reply::Result(header(100, 0x6600))).await;

    let header = harness.rpc.block_header(100).await.unwrap();

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
    let (_, harness) = over_one(|_, _| Reply::Result(Value::Null)).await;

    let refusal = harness.rpc.block_header(100).await.unwrap_err();

    assert!(
        refusal.to_string().contains("could not be read"),
        "{refusal}"
    );
}
