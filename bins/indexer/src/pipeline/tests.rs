use std::sync::Arc;

use indexing::IndexerStatus;
use tokio_util::sync::CancellationToken;

use super::{Stores, assemble};
use crate::config::Config;

#[tokio::test]
async fn follows_the_spoke_then_the_hub_at_the_configured_addresses() {
    let env = [
        ("OTEL_SERVICE_NAME", "indexer-rust"),
        ("CHAIN_ID", "1"),
        ("RPC_URLS", "https://eth.example/v2/the-providers-key"),
        (
            "MAIN_SPOKE_ADDRESS",
            "0x1111111111111111111111111111111111111111",
        ),
        (
            "CORE_HUB_ADDRESS",
            "0x2222222222222222222222222222222222222222",
        ),
    ]
    .into_iter()
    .collect();
    let stores = Stores {
        clickhouse: clickhouse_client::clickhouse::Client::default(),
        postgres: postgres::build_pool(
            "postgres://postgres@127.0.0.1:1/nothing?sslmode=disable",
            postgres::Limits::default(),
        )
        .unwrap(),
    };

    let pipeline = assemble(
        &Config::parse(&env).unwrap(),
        stores,
        &opentelemetry::global::meter("pipeline-test"),
        Arc::new(IndexerStatus::new()),
        CancellationToken::new(),
    )
    .unwrap();

    let names: Vec<_> = pipeline
        .processors
        .iter()
        .map(|processor| processor.name().to_owned())
        .collect();
    assert_eq!(names, ["aave-spoke(0x11111111)", "aave-hub(0x22222222)"]);
}
