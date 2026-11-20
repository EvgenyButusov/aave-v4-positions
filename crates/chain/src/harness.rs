//! The JSON-RPC client under test, with the in-memory exporter that reads its metrics.

use std::time::Duration;

use opentelemetry::KeyValue;
use opentelemetry::metrics::MeterProvider as _;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData, ResourceMetrics};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, PeriodicReader, SdkMeterProvider};
use serde_json::Value;
use url::Url;

use crate::config::Config;
use crate::json_rpc::JsonRpc;
use crate::scripted::{Reply, ScriptedNetwork};

pub(crate) struct Harness {
    pub(crate) rpc: JsonRpc,
    exporter: InMemoryMetricExporter,
    meters: SdkMeterProvider,
}

/// A client over one scripted provider, and the network that records what it was asked.
pub(crate) async fn over_one(
    script: impl Fn(&str, &Value) -> Reply + Send + Sync + 'static,
) -> (ScriptedNetwork, Harness) {
    let network = ScriptedNetwork::new();
    let primary = network.node("primary", script).await;
    let harness = harness(&[&primary.url]);

    (network, harness)
}

pub(crate) fn harness(urls: &[&str]) -> Harness {
    harness_within(urls, Duration::from_secs(5))
}

pub(crate) fn harness_within(urls: &[&str], timeout: Duration) -> Harness {
    let exporter = InMemoryMetricExporter::default();
    let meters = SdkMeterProvider::builder()
        .with_reader(PeriodicReader::builder(exporter.clone()).build())
        .build();
    let mut urls = urls.iter().map(|url| Url::parse(url).unwrap());
    let config = Config {
        preferred: urls.next().unwrap(),
        fallbacks: urls.collect(),
        timeout,
    };

    Harness {
        rpc: JsonRpc::connect(&config, &meters.meter("test")).unwrap(),
        exporter,
        meters,
    }
}

impl Harness {
    /// Each point of the instrument as its sorted labels and its value, or its sample count for a
    /// histogram, in label order.
    pub(crate) fn points(&self, name: &str) -> Vec<(Vec<String>, u64)> {
        self.exporter.reset();
        self.meters.force_flush().expect("a collection");

        let mut points: Vec<_> = self
            .exporter
            .get_finished_metrics()
            .expect("the exported batches")
            .iter()
            .flat_map(ResourceMetrics::scope_metrics)
            .flat_map(|scope| scope.metrics())
            .filter(|metric| metric.name() == name)
            .flat_map(|metric| match metric.data() {
                AggregatedMetrics::U64(MetricData::Sum(sum)) => sum
                    .data_points()
                    .map(|point| (labels(point.attributes()), point.value()))
                    .collect(),
                AggregatedMetrics::F64(MetricData::Histogram(histogram)) => histogram
                    .data_points()
                    .map(|point| (labels(point.attributes()), point.count()))
                    .collect(),
                _ => Vec::new(),
            })
            .collect();
        points.sort();
        points
    }
}

/// One `eth_blockNumber` call to `provider` as [`Harness::points`] reports it, with `extra` among
/// its labels when it is not empty.
pub(crate) fn head_read(provider: &str, preferred: bool, extra: &str) -> (Vec<String>, u64) {
    let mut labels = vec![
        format!("preferred={preferred}"),
        format!("provider={provider}"),
        "rpc.method=eth_blockNumber".to_owned(),
    ];
    if !extra.is_empty() {
        labels.push(extra.to_owned());
    }
    labels.sort();

    (labels, 1)
}

fn labels<'a>(attributes: impl Iterator<Item = &'a KeyValue>) -> Vec<String> {
    let mut labels: Vec<String> = attributes
        .map(|attribute| format!("{}={}", attribute.key, attribute.value.as_str()))
        .collect();
    labels.sort();
    labels
}
