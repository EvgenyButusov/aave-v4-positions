//! Both ledgers over a database of a case's own, migrated with the corpus `bins/migrate` applies,
//! so every append also runs the fold's materialized views over what it wrote.

use aave::events::{Contract, DecodedEvent, EventLedger as _};
use clickhouse_client::clickhouse::Client;
use clickhouse_client::{Config, TransportSecurity, build_client};
use units::ChainId;

use crate::ClickHouseEventLedger;

pub(crate) struct Ledgers {
    pub(crate) client: Client,
    pub(crate) chain_id: ChainId,
    pub(crate) spoke: ClickHouseEventLedger,
    pub(crate) hub: ClickHouseEventLedger,
}

impl Ledgers {
    /// Dropped and recreated rather than reused: cases run at once against one server, and every
    /// migration is `IF NOT EXISTS`, so a database left behind would hide an edited one.
    pub(crate) async fn new(case: &str) -> Self {
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
            chain_id: aave::MAIN_SPOKE_ON_ETHEREUM.chain_id,
            client,
        }
    }

    /// What the processor does with a dispatched range: retract it, then write it again.
    pub(crate) async fn index(
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

pub(crate) async fn empty_database(case: &str) -> Client {
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
pub(crate) fn at(mut event: DecodedEvent, block: u64, log_index: u32) -> DecodedEvent {
    event.block_number = block;
    event.log_index = log_index;
    event
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
