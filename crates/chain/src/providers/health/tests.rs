use indexing::ChainClient as _;
use tracing::instrument::WithSubscriber as _;
use tracing_subscriber::layer::SubscriberExt as _;

use crate::harness::harness;
use crate::scripted::{ScriptedNetwork, UNREACHABLE, at_head, at_head_except, failing};

#[tokio::test]
async fn counts_and_logs_a_failover_once_however_many_calls_it_serves() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", at_head_except(|nth| nth >= 1))
        .await;
    let secondary = network.node("secondary", at_head(100)).await;

    let harness = harness(&[&primary.url, &secondary.url]);
    let lines = logged(async {
        for _ in 0..4 {
            harness.rpc.head_block_number().await.unwrap();
        }
    })
    .await;

    assert_eq!(
        harness.points("rpc.client.failovers"),
        [(
            vec![
                format!("previous_provider={}", primary.label),
                format!("provider={}", secondary.label),
            ],
            1
        )]
    );
    assert_eq!(lines.matches("rpc provider failover").count(), 1, "{lines}");
}

#[tokio::test]
async fn does_not_report_the_first_successful_call_as_a_failover() {
    let network = ScriptedNetwork::new();
    let secondary = network.node("secondary", at_head(100)).await;

    let harness = harness(&[UNREACHABLE, &secondary.url]);
    let lines = logged(async {
        harness.rpc.head_block_number().await.unwrap();
    })
    .await;

    assert!(harness.points("rpc.client.failovers").is_empty());
    assert!(!lines.contains("rpc provider"), "{lines}");
}

#[tokio::test]
async fn logs_the_recovery_when_the_preferred_provider_answers_again() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", at_head_except(|nth| nth == 1))
        .await;
    let secondary = network.node("secondary", at_head(100)).await;

    let harness = harness(&[&primary.url, &secondary.url]);
    let lines = logged(async {
        for _ in 0..3 {
            harness.rpc.head_block_number().await.unwrap();
        }
    })
    .await;

    assert_eq!(lines.matches("rpc provider failover").count(), 1, "{lines}");
    assert_eq!(
        lines.matches("rpc provider recovered").count(),
        1,
        "{lines}"
    );
    assert_eq!(harness.points("rpc.client.failovers").len(), 1);
}

#[tokio::test]
async fn does_not_hand_over_to_a_provider_that_only_errored() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", at_head_except(|nth| nth >= 1))
        .await;
    let secondary = network.node("secondary", failing).await;

    let harness = harness(&[&primary.url, &secondary.url]);
    let lines = logged(async {
        harness.rpc.head_block_number().await.unwrap();
        let _ = harness.rpc.head_block_number().await;
    })
    .await;

    assert!(harness.points("rpc.client.failovers").is_empty());
    assert!(!lines.contains("rpc provider"), "{lines}");
}

async fn logged(work: impl Future<Output = ()>) -> String {
    test_tracing::interesting();

    let (captured, lines) = test_tracing::Captured::new();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .without_time()
            .with_writer(captured),
    );
    work.with_subscriber(subscriber).await;

    lines.read()
}
