use std::time::Duration;

use alloy_rpc_client::RpcClient;
use indexing::ChainClient as _;
use opentelemetry::metrics::MeterProvider as _;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use serde_json::Value;

use crate::harness::{harness, harness_within, head_read};
use crate::providers::{Providers, label, method_of};
use crate::scripted::{Reply, ScriptedNetwork, UNREACHABLE, at_head, failing};

/// How the primary fails, its `error.type`, and the status it is counted under, if any.
/// No reply is a port nothing listens on.
type Failure = (Option<fn() -> Reply>, &'static str, Option<u16>);

#[tokio::test]
async fn uses_the_first_provider_while_it_is_healthy_and_attributes_the_call_to_it() {
    let network = ScriptedNetwork::new();
    let primary = network.node("primary", at_head(100)).await;
    let secondary = network.node("secondary", at_head(100)).await;

    let harness = harness(&[&primary.url, &secondary.url]);
    harness.rpc.head_block_number().await.unwrap();

    assert_eq!(
        network.asked(),
        [("primary".into(), "eth_blockNumber".into())]
    );
    assert_eq!(
        harness.points("rpc.client.requests"),
        [head_read(&primary.label, true, "http.status_code=200")]
    );
    assert_eq!(
        harness.points("rpc.client.duration"),
        [head_read(&primary.label, true, "")]
    );
}

#[tokio::test]
async fn falls_over_past_each_kind_of_failure_and_counts_it_by_type() {
    let failures: [Failure; 6] = [
        (Some(|| Reply::Status(500)), "500", Some(500)),
        (
            Some(|| Reply::RateLimited { retry_after: 30 }),
            "429",
            Some(429),
        ),
        (
            Some(|| Reply::Error {
                code: -32005,
                message: "rate limited",
            }),
            "-32005",
            Some(200),
        ),
        (
            Some(|| Reply::NotJsonRpc("<html>502 Bad Gateway</html>")),
            "deserialize",
            None,
        ),
        (Some(|| Reply::Hang), "timeout", None),
        (None, "connect", None),
    ];

    for (reply, error_type, status) in failures {
        let network = ScriptedNetwork::new();
        let (url, label) = match reply {
            Some(reply) => {
                let primary = network.node("primary", move |_, _| reply()).await;
                (primary.url, primary.label)
            }
            None => (UNREACHABLE.to_owned(), "127.0.0.1:1".to_owned()),
        };
        let secondary = network.node("secondary", at_head(100)).await;
        let harness = harness_within(&[&url, &secondary.url], Duration::from_millis(200));

        assert_eq!(
            harness.rpc.head_block_number().await.unwrap(),
            100,
            "{error_type}"
        );
        assert_eq!(
            harness.points("rpc.client.errors"),
            [head_read(&label, true, &format!("error.type={error_type}"))],
            "{error_type}"
        );
        let mut requests = vec![head_read(&secondary.label, false, "http.status_code=200")];
        requests.extend(
            status.map(|status| head_read(&label, true, &format!("http.status_code={status}"))),
        );
        requests.sort();
        assert_eq!(
            harness.points("rpc.client.requests"),
            requests,
            "{error_type}"
        );
    }
}

#[tokio::test]
async fn surfaces_a_total_outage_after_one_sweep_of_the_list() {
    let network = ScriptedNetwork::new();
    let primary = network.node("primary", failing).await;
    let secondary = network.node("secondary", failing).await;

    let harness = harness(&[&primary.url, &secondary.url]);

    assert!(harness.rpc.head_block_number().await.is_err());
    assert_eq!(network.asked().len(), 2);
}

#[tokio::test]
async fn takes_a_revert_as_the_answer_rather_than_asking_the_next_provider() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node("primary", |_, _| Reply::Error {
            code: 3,
            message: "execution reverted",
        })
        .await;
    let secondary = network.node("secondary", at_head(100)).await;

    let providers = Providers::new(
        &url::Url::parse(&primary.url).unwrap(),
        &[url::Url::parse(&secondary.url).unwrap()],
        &crate::http::client(Duration::from_secs(5)).unwrap(),
        &SdkMeterProvider::default().meter("test"),
    );
    let answer = RpcClient::new(providers, false)
        .request::<_, Value>("eth_call", ())
        .await;

    assert!(answer.is_err());
    assert_eq!(network.asked(), [("primary".into(), "eth_call".into())]);
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
