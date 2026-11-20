//! Both ledgers over a database of a case's own, migrated with the corpus `bins/migrate` applies,
//! so every append also runs the fold's materialized views over what it wrote.

use clickhouse_client::clickhouse::{self, Client};
use clickhouse_client::{Config, TransportSecurity, build_client};
use serde::Deserialize;
use serde_json::Value;
use units::ChainId;

use crate::events::{ClickHouseEventLedger, Contract, DecodedEvent};

pub(super) struct Ledgers {
    client: Client,
    pub(super) chain_id: ChainId,
    pub(super) spoke: ClickHouseEventLedger,
    pub(super) hub: ClickHouseEventLedger,
}

#[derive(clickhouse::Row, Deserialize)]
pub(super) struct Live {
    pub(super) block_number: u64,
    pub(super) log_index: u32,
    pub(super) event_name: String,
    pub(super) body: String,
}

impl Ledgers {
    /// Dropped and recreated rather than reused: cases run at once against one server, and every
    /// migration is `IF NOT EXISTS`, so a database left behind would hide an edited one.
    pub(super) async fn new(case: &str) -> Self {
        let client = empty_database(case).await;
        for migration in migrations::CLICKHOUSE
            .iter()
            .flat_map(|source| source.files)
        {
            client.query(migration.sql).execute().await.unwrap();
        }

        Self {
            spoke: ClickHouseEventLedger::new(client.clone(), Contract::Spoke),
            hub: ClickHouseEventLedger::new(client.clone(), Contract::Hub),
            chain_id: crate::MAIN_SPOKE_ON_ETHEREUM.chain_id,
            client,
        }
    }

    /// What the processor does with a dispatched range: retract it, then write it again.
    pub(super) async fn index(
        &self,
        contract: Contract,
        from: u64,
        to: u64,
        events: &[DecodedEvent],
    ) {
        let ledger = self.ledger(contract);
        ledger.revert(self.chain_id, from, to).await.unwrap();
        ledger.append(events).await.unwrap();
    }

    fn ledger(&self, contract: Contract) -> &ClickHouseEventLedger {
        match contract {
            Contract::Spoke => &self.spoke,
            Contract::Hub => &self.hub,
        }
    }
}

pub(super) async fn empty_database(case: &str) -> Client {
    let database = format!("rust_events_{case}");
    let bootstrap = build_client(config("default")).unwrap();
    for statement in [
        format!("DROP DATABASE IF EXISTS {database} SYNC"),
        format!("CREATE DATABASE {database}"),
    ] {
        bootstrap.query(&statement).execute().await.unwrap();
    }

    build_client(config(&database)).unwrap()
}

/// `event` moved to `block` and `log_index`, its body untouched so the fold's views still accept it.
pub(super) fn at(mut event: DecodedEvent, block: u64, log_index: u32) -> DecodedEvent {
    event.block_number = block;
    event.log_index = log_index;
    event
}

pub(super) async fn live(ledgers: &Ledgers, contract: Contract) -> Vec<Live> {
    ledgers
        .client
        .query(&format!(
            "SELECT block_number, log_index, event_name, body FROM {} ORDER BY block_number, log_index",
            contract.view()
        ))
        .fetch_all::<Live>()
        .await
        .unwrap()
}

/// Every row of the table itself, retractions included, as JSON.
pub(super) async fn raw(ledgers: &Ledgers, contract: Contract) -> Vec<Value> {
    let mut cursor = ledgers
        .client
        .query(&format!(
            "SELECT * FROM {} FORMAT JSONEachRow",
            contract.table()
        ))
        .fetch_bytes("JSONEachRow")
        .unwrap();
    let bytes = cursor.collect().await.unwrap();

    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn config(database: &str) -> Config {
    Config {
        url: std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://localhost:8123".to_owned()),
        transport_security: TransportSecurity::AllowInsecureHttp,
        database: database.to_owned(),
        user: std::env::var("CLICKHOUSE_USER").unwrap_or_else(|_| "default".to_owned()),
        password: std::env::var("CLICKHOUSE_PASSWORD").unwrap_or_default(),
        max_execution: std::time::Duration::from_secs(60),
    }
}
