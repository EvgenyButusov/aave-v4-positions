mod row;
#[cfg(test)]
mod tests;

use clickhouse_client::clickhouse::{self, Client};
use units::ChainId;

use crate::events::{Contract, DecodedEvent};

use row::{Row, version_now};

/// Every column a retraction reproduces. A materialized view sees each inserted row, so a
/// retraction missing one subtracts from the wrong group while the ledger itself still nets to zero.
const COLUMNS: &str = "chain_id, address, block_number, log_index, block_hash, block_timestamp, \
                       tx_hash, tx_index, event_name, topic1, topic2, topic3, body, data";

/// One contract's ledger, written by appending only: a range is emptied by inserting the negation
/// of its live rows, never by a `DELETE`.
///
/// **[`revert`](Self::revert) the range, then [`append`](Self::append), every time it is indexed.**
/// Dispatch is at least once, and the engine collapses no repeated insert on its own, so an append
/// without the revert before it leaves two live copies of the range.
#[derive(Clone)]
pub struct ClickHouseEventLedger {
    client: Client,
    contract: Contract,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the {contract} ledger could not retract blocks {from}..={to}")]
    RevertFailed {
        contract: Contract,
        from: u64,
        to: u64,
        #[source]
        source: clickhouse::error::Error,
    },

    #[error("the {contract} ledger could not append {events} events")]
    AppendFailed {
        contract: Contract,
        events: usize,
        #[source]
        source: clickhouse::error::Error,
    },
}

impl ClickHouseEventLedger {
    #[must_use]
    pub fn new(client: Client, contract: Contract) -> Self {
        Self { client, contract }
    }

    /// Writes each live row in `from..=to` back with its sign flipped, carrying its version, so the
    /// pair collapses. Copied server side from the view, which is the one way to be sure every column
    /// and the version come back as they were. An empty range is a no-op.
    ///
    /// # Errors
    ///
    /// [`Error::RevertFailed`] when ClickHouse refuses the statement.
    pub async fn revert(&self, chain_id: ChainId, from: u64, to: u64) -> Result<(), Error> {
        let (table, view) = (self.contract.table(), self.contract.view());

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
                source,
            })
    }

    /// Adds `events` as live rows, all under one version. Appending none is a no-op.
    ///
    /// # Errors
    ///
    /// [`Error::AppendFailed`] when ClickHouse refuses the insert.
    pub async fn append(&self, events: &[DecodedEvent]) -> Result<(), Error> {
        if events.is_empty() {
            return Ok(());
        }

        self.insert(events)
            .await
            .map_err(|source| Error::AppendFailed {
                contract: self.contract,
                events: events.len(),
                source,
            })
    }

    async fn insert(&self, events: &[DecodedEvent]) -> Result<(), clickhouse::error::Error> {
        let version = version_now();
        let mut insert = self.client.insert::<Row<'_>>(self.contract.table()).await?;
        for event in events {
            insert.write(&Row::live(event, version)).await?;
        }
        insert.end().await
    }
}
