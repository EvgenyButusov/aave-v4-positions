//! The JSON-RPC client under test, with the in-memory exporter that reads its metrics.

use std::time::Duration;

use opentelemetry::metrics::MeterProvider as _;
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, PeriodicReader, SdkMeterProvider};

use crate::config::Config;
use crate::json_rpc::JsonRpc;

pub(crate) struct Harness {
    pub(crate) rpc: JsonRpc,
    pub(crate) exporter: InMemoryMetricExporter,
    pub(crate) meters: SdkMeterProvider,
}

pub(crate) fn harness(urls: &[&str]) -> Harness {
    harness_within(urls, Duration::from_secs(5))
}

pub(crate) fn harness_within(urls: &[&str], timeout: Duration) -> Harness {
    let exporter = InMemoryMetricExporter::default();
    let meters = SdkMeterProvider::builder()
        .with_reader(PeriodicReader::builder(exporter.clone()).build())
        .build();
    let config = Config {
        urls: urls.iter().map(|url| (*url).to_owned()).collect(),
        timeout,
    };

    Harness {
        rpc: JsonRpc::connect(&config, &meters.meter("test")).unwrap(),
        exporter,
        meters,
    }
}
