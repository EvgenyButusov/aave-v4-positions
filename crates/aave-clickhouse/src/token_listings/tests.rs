use aave::events::fixtures::event;
use aave::events::{Contract, DecodedEvent, EventLedger as _};
use alloy_primitives::{Address, address};
use clickhouse_client::clickhouse::Client;
use serde_json::json;
use token_metadata::TokenListings as _;
use units::ChainId;

use super::ClickHouseTokenListings;
use crate::harness::{Ledgers, at};

const USDC: Address = address!("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");
const WETH: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");

/// The captured `AddAsset`, relisting `underlying` under `asset_id`. Checksummed, as the decoder
/// writes it, so the fold's `lower()` is what the result depends on.
fn add_asset(block: u64, log: u32, asset_id: u64, underlying: Address) -> DecodedEvent {
    let mut listing = at(event(Contract::Hub, "AddAsset"), block, log);
    listing.body = json!({
        "assetId": asset_id.to_string(),
        "underlying": underlying.to_checksum(None),
        "decimals": 6,
    })
    .to_string();
    listing
}

/// `UpdateAsset` writes a NULL `underlying` every time it fires.
fn checkpoints(from: u64, count: u64) -> Vec<DecodedEvent> {
    (from..from + count)
        .map(|block| at(event(Contract::Hub, "UpdateAsset"), block, 0))
        .collect()
}

async fn listed(ledgers: &Ledgers, events: &[DecodedEvent]) -> Vec<Address> {
    ledgers.hub.append(events).await.unwrap();
    all(ledgers, ledgers.chain_id).await
}

async fn all(ledgers: &Ledgers, chain_id: ChainId) -> Vec<Address> {
    let mut tokens = ClickHouseTokenListings::new(ledgers.client.clone())
        .all(chain_id)
        .await
        .unwrap();
    tokens.sort();
    tokens
}

#[tokio::test]
async fn finds_every_underlying_the_hub_has_listed() {
    let ledgers = Ledgers::new("listings_every").await;

    let tokens = listed(
        &ledgers,
        &[
            add_asset(24_722_784, 0, 0, USDC),
            add_asset(24_722_784, 1, 1, WETH),
        ],
    )
    .await;

    assert_eq!(tokens, [USDC, WETH]);
}

#[tokio::test]
async fn reports_one_token_however_many_checkpoints_its_asset_has() {
    let ledgers = Ledgers::new("listings_checkpoints").await;
    let mut events = vec![add_asset(24_722_784, 0, 0, USDC)];
    events.extend(checkpoints(24_730_000, 40));

    assert_eq!(listed(&ledgers, &events).await, [USDC]);
}

#[tokio::test]
async fn reports_a_token_listed_under_two_asset_ids_once() {
    let ledgers = Ledgers::new("listings_twice").await;

    let tokens = listed(
        &ledgers,
        &[
            add_asset(24_722_784, 0, 0, USDC),
            add_asset(24_722_784, 1, 1, USDC),
        ],
    )
    .await;

    assert_eq!(tokens, [USDC]);
}

#[tokio::test]
async fn finds_nothing_on_a_chain_it_has_never_indexed() {
    let ledgers = Ledgers::new("listings_other_chain").await;
    listed(&ledgers, &[add_asset(24_722_784, 0, 0, USDC)]).await;

    assert!(all(&ledgers, ChainId::known(8453)).await.is_empty());
}

#[tokio::test]
async fn still_reports_a_listing_the_ledger_has_retracted() {
    let ledgers = Ledgers::new("listings_retracted").await;
    ledgers
        .hub
        .append(&[add_asset(25_000_000, 0, 0, USDC)])
        .await
        .unwrap();
    ledgers
        .hub
        .revert(ledgers.chain_id, 25_000_000, 25_000_000)
        .await
        .unwrap();

    assert_eq!(all(&ledgers, ledgers.chain_id).await, [USDC]);
}

#[tokio::test]
async fn skips_a_listing_whose_body_names_no_underlying() {
    let ledgers = Ledgers::new("listings_no_underlying").await;
    let mut unnamed = add_asset(24_722_784, 1, 1, WETH);
    unnamed.body = json!({ "assetId": "1", "decimals": 6 }).to_string();

    let tokens = listed(&ledgers, &[add_asset(24_722_784, 0, 0, USDC), unnamed]).await;

    assert_eq!(tokens, [USDC]);
}

#[tokio::test]
async fn answers_from_the_projection_rather_than_scanning_the_column() {
    let ledgers = Ledgers::new("listings_projection").await;
    let mut events = vec![add_asset(24_722_784, 0, 0, USDC)];
    events.extend(checkpoints(24_730_000, 200));
    listed(&ledgers, &events).await;

    assert!(
        projections_of_the_last_listing(&ledgers.client)
            .await
            .contains("listed_tokens")
    );
}

async fn projections_of_the_last_listing(client: &Client) -> String {
    client.query("SYSTEM FLUSH LOGS").execute().await.unwrap();

    client
        .query(
            "SELECT arrayStringConcat(projections, ',') \
             FROM system.query_log \
             WHERE type = 'QueryFinish' \
               AND current_database = currentDatabase() \
               AND query LIKE 'SELECT DISTINCT underlying%' \
             ORDER BY event_time_microseconds DESC \
             LIMIT 1",
        )
        .fetch_one::<String>()
        .await
        .unwrap()
}
