use std::time::{SystemTime, UNIX_EPOCH};

use aave::events::DecodedEvent;
use clickhouse_client::clickhouse;
use serde::Serialize;

/// One row of either ledger, in the textual forms the TypeScript wrote: lower-case hex throughout.
#[derive(clickhouse::Row, Serialize)]
pub(super) struct Row<'a> {
    chain_id: u64,
    address: String,
    block_number: u64,
    log_index: u32,
    block_hash: String,
    block_timestamp: u32,
    tx_hash: String,
    tx_index: u32,
    event_name: &'a str,
    topic1: Option<String>,
    topic2: Option<String>,
    topic3: Option<String>,
    body: &'a str,
    data: String,
    /// Half the pairing key: it only has to differ between successive states of the same log.
    version: u64,
    sign: i8,
}

impl<'a> Row<'a> {
    pub(super) fn live(event: &'a DecodedEvent, version: u64) -> Self {
        let [topic1, topic2, topic3] = event
            .topics
            .map(|topic| topic.map(|word| format!("{word:#x}")));

        Self {
            chain_id: event.chain_id.get(),
            address: format!("{:#x}", event.address),
            block_number: event.block_number,
            log_index: event.log_index,
            block_hash: format!("{:#x}", event.block_hash),
            block_timestamp: event.block_timestamp,
            tx_hash: format!("{:#x}", event.transaction_hash),
            tx_index: event.transaction_index,
            event_name: event.event_name,
            topic1,
            topic2,
            topic3,
            body: &event.body,
            data: event.data.to_string(),
            version,
            sign: 1,
        }
    }
}

/// Milliseconds since the epoch, one stamp per append: two dispatches of a range are at least an
/// RPC round trip apart.
pub(super) fn version_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}
