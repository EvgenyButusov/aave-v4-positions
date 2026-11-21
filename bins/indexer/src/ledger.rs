mod row;
#[cfg(test)]
mod tests;

use aave::events::{Contract, DecodedEvent, Error, EventLedger};
use async_trait::async_trait;
use clickhouse_client::clickhouse::{self, Client};
use units::ChainId;

use row::{Row, version_now};

/// Every column a retraction reproduces. A materialized view sees each inserted row, so a
/// retraction missing one subtracts from the wrong group while the ledger itself still nets to zero.
const COLUMNS: &str = "chain_id, address, block_number, log_index, block_hash, block_timestamp, \
                       tx_hash, tx_index, event_name, topic1, topic2, topic3, body, data";

/// A range is emptied by inserting the negation of its live rows, never by a `DELETE`. The engine
/// collapses no repeated insert on its own, which is why the port asks for a revert first.
#[derive(Clone)]
pub(crate) struct ClickHouseEventLedger {
    client: Client,
    contract: Contract,
}

impl ClickHouseEventLedger {
    pub(crate) fn new(client: Client, contract: Contract) -> Self {
        Self { client, contract }
    }

    async fn insert(&self, events: &[DecodedEvent]) -> Result<(), clickhouse::error::Error> {
        let version = version_now();
        let mut insert = self.client.insert::<Row<'_>>(table(self.contract)).await?;
        for event in events {
            insert.write(&Row::live(event, version)).await?;
        }
        insert.end().await
    }
}

#[async_trait]
impl EventLedger for ClickHouseEventLedger {
    /// Copied server side from the view, carrying each row's version with its sign flipped, which is
    /// the one way to be sure every column comes back as it was and the pair collapses.
    async fn revert(&self, chain_id: ChainId, from: u64, to: u64) -> Result<(), Error> {
        let (table, view) = (table(self.contract), view(self.contract));

        self.client
            .query(&format!(
                "INSERT INTO {table} ({COLUMNS}, version, sign) \
                 SELECT {COLUMNS}, version, -1 FROM {view} \
                 WHERE chain_id = {{chainId:UInt64}} \
                 AND block_number BETWEEN {{from:UInt64}} AND {{to:UInt64}}"
            ))
            .param("chainId", chain_id.get())
            .param("from", from)
            .param("to", to)
            .execute()
            .await
            .map_err(|source| Error::RevertFailed {
                contract: self.contract,
                from,
                to,
                source: source.into(),
            })
    }

    /// All under one version.
    async fn append(&self, events: &[DecodedEvent]) -> Result<(), Error> {
        if events.is_empty() {
            return Ok(());
        }

        self.insert(events)
            .await
            .map_err(|source| Error::AppendFailed {
                contract: self.contract,
                events: events.len(),
                source: source.into(),
            })
    }
}

fn table(contract: Contract) -> &'static str {
    match contract {
        Contract::Spoke => "spoke_events",
        Contract::Hub => "hub_events",
    }
}

/// Every read goes through this: an unpaired retraction is still visible in the table.
fn view(contract: Contract) -> &'static str {
    match contract {
        Contract::Spoke => "spoke_events_current",
        Contract::Hub => "hub_events_current",
    }
}
