//! A migrated database, a store over it, and the events put into it.
//!
//! Not fixtures — those are [`crate::store::fixtures`], and they are data. This
//! is what stands a scenario up.
//!
//! Seeded through the fold, not around it: these append decoded events and let
//! the materialized views do the work, so the specs are store specs rather
//! than assertions about a hand-written table.

use aave::events::{Contract, DecodedEvent, EventLedger as _};
use aave_clickhouse::ClickHouseEventLedger;
use alloy_primitives::{B256, Bytes};
use clickhouse_client::clickhouse::Client;
use clickhouse_client::{Config, build_client};

use super::ClickHousePositionStore;
use crate::store::fixtures::{CHAIN_ID, Event};

// The projections read every field out of `body`, so the hashes and topics stay placeholders.
fn decoded(event: &Event) -> DecodedEvent {
    DecodedEvent {
        chain_id: CHAIN_ID,
        address: event.address,
        block_number: event.at.block,
        block_hash: B256::repeat_byte(0xaa),
        block_timestamp: event.timestamp,
        transaction_hash: B256::repeat_byte(0xbb),
        transaction_index: 0,
        log_index: event.at.log,
        event_name: event.name,
        topics: [None; 3],
        body: event.body.clone(),
        data: Bytes::new(),
    }
}

pub(super) async fn append(client: &Client, contract: Contract, events: &[Event]) {
    let events: Vec<DecodedEvent> = events.iter().map(decoded).collect();
    ClickHouseEventLedger::new(client.clone(), contract)
        .append(&events)
        .await
        .unwrap();
}

fn config(database: &str) -> Config {
    Config {
        url: std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://localhost:8123".to_owned()),
        transport_security: clickhouse_client::TransportSecurity::AllowInsecureHttp,
        database: database.to_owned(),
        user: std::env::var("CLICKHOUSE_USER").unwrap_or_else(|_| "default".to_owned()),
        password: std::env::var("CLICKHOUSE_PASSWORD").unwrap_or_default(),
        max_execution: std::time::Duration::from_secs(60),
    }
}

/// A migrated database of the caller's own.
///
/// One per test, not one per file: `cargo` runs these concurrently against one
/// server and they all write the same table names.
///
/// Dropped first rather than `IF NOT EXISTS`, which every migration is written
/// with — so against a database an earlier run left behind the whole set is a
/// no-op and editing a migration has no local effect.
///
/// `SYNC`, because an Atomic database's drop otherwise reclaims its metadata
/// 480 seconds later, which a suite re-run every few minutes outpaces.
pub(super) async fn migrated_database(name: &str) -> Client {
    let bootstrap = build_client(config("default")).unwrap();
    bootstrap
        .query(&format!("DROP DATABASE IF EXISTS {name} SYNC"))
        .execute()
        .await
        .unwrap();
    bootstrap
        .query(&format!("CREATE DATABASE {name}"))
        .execute()
        .await
        .unwrap();

    // The corpus `bins/migrate` applies, in its order, rather than a second
    // reading of the directories. No runner: the database is empty.
    let client = build_client(config(name)).unwrap();
    for embedded in migrations::CLICKHOUSE
        .iter()
        .flat_map(|source| source.files)
    {
        client.query(embedded.sql).execute().await.unwrap();
    }
    client
}

pub(super) async fn store(test: &str) -> (ClickHousePositionStore, Client) {
    let client = migrated_database(&format!("rust_positions_{test}")).await;
    (ClickHousePositionStore::new(client.clone()), client)
}

/// A dispatched range, as the processor leaves it. Append only: every test
/// reads a database created empty, so a retraction has nothing to cancel.
pub(super) async fn index(client: &Client, batch: &[Event]) {
    append(client, Contract::Spoke, batch).await;
}
