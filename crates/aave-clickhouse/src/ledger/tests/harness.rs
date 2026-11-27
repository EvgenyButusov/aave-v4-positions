//! What the ledger cases read back: the view's live rows, and the table's own.

use aave::events::Contract;
use clickhouse_client::clickhouse;
use serde::Deserialize;
use serde_json::Value;

use crate::harness::Ledgers;
use crate::ledger::{table, view};

#[derive(clickhouse::Row, Deserialize)]
pub(super) struct Live {
    pub(super) block_number: u64,
    pub(super) log_index: u32,
    pub(super) event_name: String,
    pub(super) body: String,
}

pub(super) async fn live(ledgers: &Ledgers, contract: Contract) -> Vec<Live> {
    ledgers
        .client
        .query(&format!(
            "SELECT block_number, log_index, event_name, body FROM {} ORDER BY block_number, log_index",
            view(contract)
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
            table(contract)
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
