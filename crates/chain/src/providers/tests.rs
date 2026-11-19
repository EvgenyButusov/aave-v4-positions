use std::time::Duration;

use alloy_rpc_client::RpcClient;
use indexing::ChainClient as _;
use opentelemetry::KeyValue;
use opentelemetry::metrics::MeterProvider as _;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData, ResourceMetrics};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, PeriodicReader, SdkMeterProvider};
use serde_json::Value;
use tracing::instrument::WithSubscriber as _;
use tracing_subscriber::layer::SubscriberExt as _;

use crate::node::{Client, Network, Reply, UNREACHABLE, client, client_within, hex, in_turn};
use crate::providers::{Providers, label, method_of};

fn head(height: u64) -> impl Fn(&str, &Value) -> Reply + Send + Sync + 'static {
    move |_, _| Reply::Result(hex(height))
}

fn failing(_: &str, _: &Value) -> Reply {
    Reply::Status(500)
}

#[tokio::test]
async fn uses_the_first_provider_while_it_is_healthy() {
    let network = Network::new();
    let primary = network.node("primary", head(100)).await;
    let secondary = network.node("secondary", head(100)).await;

    let client = client(&[&primary.url, &secondary.url]);
    client.rpc.head_block_number().await.unwrap();

    assert_eq!(
        network.asked(),
        [("primary".into(), "eth_blockNumber".into())]
    );
}

#[tokio::test]
async fn falls_over_to_the_next_provider_when_the_first_is_down() {
    let network = Network::new();
    let secondary = network.node("secondary", head(100)).await;

    let client = client(&[UNREACHABLE, &secondary.url]);

    assert_eq!(client.rpc.head_block_number().await.unwrap(), 100);
}

#[tokio::test]
async fn gives_a_failing_provider_exactly_one_attempt_before_falling_over() {
    let network = Network::new();
    let primary = network.node("primary", failing).await;
    let secondary = network.node("secondary", head(100)).await;

    let client = client(&[&primary.url, &secondary.url]);

    assert_eq!(client.rpc.head_block_number().await.unwrap(), 100);
    assert_eq!(
        network.asked(),
        [
            ("primary".into(), "eth_blockNumber".into()),
            ("secondary".into(), "eth_blockNumber".into()),
        ]
    );
}

#[tokio::test]
async fn surfaces_a_total_outage_after_one_sweep_of_the_list() {
    let network = Network::new();
    let primary = network.node("primary", failing).await;
    let secondary = network.node("secondary", failing).await;

    let client = client(&[&primary.url, &secondary.url]);

    assert!(client.rpc.head_block_number().await.is_err());
    assert_eq!(network.asked().len(), 2);
}

#[tokio::test]
async fn falls_over_on_a_json_rpc_refusal_as_on_a_transport_failure() {
    let network = Network::new();
    let primary = network
        .node("primary", |_, _| Reply::Error {
            code: -32005,
            message: "rate limited",
        })
        .await;
    let secondary = network.node("secondary", head(100)).await;

    let client = client(&[&primary.url, &secondary.url]);

    assert_eq!(client.rpc.head_block_number().await.unwrap(), 100);
}

#[tokio::test]
async fn takes_a_revert_as_the_answer_rather_than_asking_the_next_provider() {
    let network = Network::new();
    let primary = network
        .node("primary", |_, _| Reply::Error {
            code: 3,
            message: "execution reverted",
        })
        .await;
    let secondary = network.node("secondary", head(100)).await;

    let (_exporter, meters) = meters();
    let providers = Providers::new(
        &[primary.url, secondary.url],
        &crate::http::client(Duration::from_secs(5)).unwrap(),
        &meters.meter("test"),
    )
    .unwrap();
    let answer = RpcClient::new(providers, false)
        .request::<_, Value>("eth_call", ())
        .await;

    assert!(answer.is_err());
    assert_eq!(network.asked(), [("primary".into(), "eth_call".into())]);
}

#[tokio::test]
async fn a_provider_that_never_answers_is_abandoned_at_the_timeout() {
    let network = Network::new();
    let primary = network.node("primary", |_, _| Reply::Hang).await;
    let secondary = network.node("secondary", head(100)).await;

    let client = client_within(&[&primary.url, &secondary.url], Duration::from_millis(200));

    assert_eq!(client.rpc.head_block_number().await.unwrap(), 100);
    assert_eq!(
        points(&client, "rpc.client.errors"),
        [(
            attributes(
                &primary.label,
                true,
                "eth_blockNumber",
                "error.type=timeout"
            ),
            1
        )]
    );
}

#[tokio::test]
async fn attributes_a_request_to_its_provider_and_method() {
    let network = Network::new();
    let primary = network.node("primary", head(100)).await;

    let client = client(&[&primary.url]);
    client.rpc.head_block_number().await.unwrap();

    assert_eq!(
        points(&client, "rpc.client.requests"),
        [(
            attributes(
                &primary.label,
                true,
                "eth_blockNumber",
                "http.status_code=200"
            ),
            1
        )]
    );
    assert_eq!(
        histogram_labels(&client, "rpc.client.duration"),
        [vec![
            "preferred=true".to_owned(),
            format!("provider={}", primary.label),
            "rpc.method=eth_blockNumber".to_owned(),
        ]]
    );
}

#[tokio::test]
async fn counts_a_refused_status_as_a_request_and_an_error() {
    let network = Network::new();
    let primary = network.node("primary", |_, _| Reply::Status(429)).await;

    let client = client(&[&primary.url]);
    let _ = client.rpc.head_block_number().await;

    assert_eq!(
        points(&client, "rpc.client.requests"),
        [(
            attributes(
                &primary.label,
                true,
                "eth_blockNumber",
                "http.status_code=429"
            ),
            1
        )]
    );
    assert_eq!(
        points(&client, "rpc.client.errors"),
        [(
            attributes(&primary.label, true, "eth_blockNumber", "error.type=429"),
            1
        )]
    );
}

#[tokio::test]
async fn counts_a_json_rpc_refusal_under_its_code() {
    let network = Network::new();
    let primary = network
        .node("primary", |_, _| Reply::Error {
            code: -32005,
            message: "rate limited",
        })
        .await;

    let client = client(&[&primary.url]);
    let _ = client.rpc.head_block_number().await;

    assert_eq!(
        points(&client, "rpc.client.errors"),
        [(
            attributes(&primary.label, true, "eth_blockNumber", "error.type=-32005"),
            1
        )]
    );
}

#[tokio::test]
async fn counts_a_refused_connection_as_an_error_and_still_fails() {
    let client = client(&[UNREACHABLE]);

    assert!(client.rpc.head_block_number().await.is_err());
    assert_eq!(
        points(&client, "rpc.client.errors"),
        [(
            attributes("127.0.0.1:1", true, "eth_blockNumber", "error.type=connect"),
            1
        )]
    );
    assert!(points(&client, "rpc.client.requests").is_empty());
}

#[tokio::test]
async fn counts_and_logs_a_failover_once_however_many_calls_it_serves() {
    let network = Network::new();
    let primary = network
        .node(
            "primary",
            in_turn(|nth| {
                if nth == 0 {
                    Reply::Result(hex(100))
                } else {
                    Reply::Status(503)
                }
            }),
        )
        .await;
    let secondary = network.node("secondary", head(100)).await;

    let client = client(&[&primary.url, &secondary.url]);
    let lines = logged(async {
        for _ in 0..4 {
            client.rpc.head_block_number().await.unwrap();
        }
    })
    .await;

    assert_eq!(
        points(&client, "rpc.client.failovers"),
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
    let network = Network::new();
    let secondary = network.node("secondary", head(100)).await;

    let client = client(&[UNREACHABLE, &secondary.url]);
    let lines = logged(async {
        client.rpc.head_block_number().await.unwrap();
    })
    .await;

    assert!(points(&client, "rpc.client.failovers").is_empty());
    assert!(!lines.contains("rpc provider"), "{lines}");
}

#[tokio::test]
async fn logs_the_recovery_when_the_preferred_provider_answers_again() {
    let network = Network::new();
    let primary = network
        .node(
            "primary",
            in_turn(|nth| {
                if nth == 1 {
                    Reply::Status(503)
                } else {
                    Reply::Result(hex(100))
                }
            }),
        )
        .await;
    let secondary = network.node("secondary", head(100)).await;

    let client = client(&[&primary.url, &secondary.url]);
    let lines = logged(async {
        for _ in 0..3 {
            client.rpc.head_block_number().await.unwrap();
        }
    })
    .await;

    assert_eq!(lines.matches("rpc provider failover").count(), 1, "{lines}");
    assert_eq!(
        lines.matches("rpc provider recovered").count(),
        1,
        "{lines}"
    );
    assert_eq!(points(&client, "rpc.client.failovers").len(), 1);
}

#[tokio::test]
async fn does_not_hand_over_to_a_provider_that_only_errored() {
    let network = Network::new();
    let primary = network
        .node(
            "primary",
            in_turn(|nth| {
                if nth == 0 {
                    Reply::Result(hex(100))
                } else {
                    Reply::Status(503)
                }
            }),
        )
        .await;
    let secondary = network.node("secondary", failing).await;

    let client = client(&[&primary.url, &secondary.url]);
    let lines = logged(async {
        client.rpc.head_block_number().await.unwrap();
        let _ = client.rpc.head_block_number().await;
    })
    .await;

    assert!(points(&client, "rpc.client.failovers").is_empty());
    assert!(!lines.contains("rpc provider"), "{lines}");
}

#[test]
fn labels_a_provider_by_host_so_its_key_never_reaches_a_metric() {
    let keyed = url::Url::parse("https://eth.example/v2/the-providers-key").unwrap();
    let ported = url::Url::parse("http://localhost:8545/?key=secret").unwrap();

    assert_eq!(label(&keyed), "eth.example");
    assert_eq!(label(&ported), "localhost:8545");
}

#[test]
fn calls_a_batch_a_batch_rather_than_naming_it_after_its_first_call() {
    let request = |method: &'static str, id: u64| {
        alloy_json_rpc::Request::new(method, alloy_json_rpc::Id::Number(id), ())
            .serialize()
            .unwrap()
    };

    let single = alloy_json_rpc::RequestPacket::Single(request("eth_getLogs", 1));
    let batch = alloy_json_rpc::RequestPacket::Batch(vec![
        request("eth_getLogs", 1),
        request("eth_blockNumber", 2),
    ]);

    assert_eq!(method_of(&single), "eth_getLogs");
    assert_eq!(method_of(&batch), "batch");
}

fn meters() -> (InMemoryMetricExporter, SdkMeterProvider) {
    let exporter = InMemoryMetricExporter::default();
    let meters = SdkMeterProvider::builder()
        .with_reader(PeriodicReader::builder(exporter.clone()).build())
        .build();

    (exporter, meters)
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

fn attributes(provider: &str, preferred: bool, method: &str, extra: &str) -> Vec<String> {
    let mut labels = vec![
        extra.to_owned(),
        format!("preferred={preferred}"),
        format!("provider={provider}"),
        format!("rpc.method={method}"),
    ];
    labels.sort();
    labels
}

fn labels<'a>(attributes: impl Iterator<Item = &'a KeyValue>) -> Vec<String> {
    let mut labels: Vec<String> = attributes
        .map(|attribute| format!("{}={}", attribute.key, attribute.value.as_str()))
        .collect();
    labels.sort();
    labels
}

fn collected(client: &Client) -> Vec<ResourceMetrics> {
    client.exporter.reset();
    client.meters.force_flush().expect("a collection");
    client
        .exporter
        .get_finished_metrics()
        .expect("the exported batches")
}

fn points(client: &Client, name: &str) -> Vec<(Vec<String>, u64)> {
    collected(client)
        .iter()
        .flat_map(ResourceMetrics::scope_metrics)
        .flat_map(|scope| scope.metrics())
        .filter(|metric| metric.name() == name)
        .flat_map(|metric| match metric.data() {
            AggregatedMetrics::U64(MetricData::Sum(sum)) => sum
                .data_points()
                .map(|point| (labels(point.attributes()), point.value()))
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

fn histogram_labels(client: &Client, name: &str) -> Vec<Vec<String>> {
    collected(client)
        .iter()
        .flat_map(ResourceMetrics::scope_metrics)
        .flat_map(|scope| scope.metrics())
        .filter(|metric| metric.name() == name)
        .flat_map(|metric| match metric.data() {
            AggregatedMetrics::F64(MetricData::Histogram(histogram)) => histogram
                .data_points()
                .map(|point| labels(point.attributes()))
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}
