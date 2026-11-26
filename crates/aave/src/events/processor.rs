#[cfg(test)]
mod tests;

use std::sync::Arc;

use alloy_primitives::{Address, B256};
use async_trait::async_trait;
use indexing::{BlockProcessor, ChainError, Failure, LogFilter, LogReader, Outcome};
use tokio_util::sync::CancellationToken;
use units::ChainId;

use crate::events::{Contract, Decoder, EventLedger};

/// Reads one contract's followed events over the dispatched range and writes them to its ledger.
/// A second Spoke is a second processor with its own address and ledger.
pub struct AaveEventProcessor {
    name: String,
    chain_id: ChainId,
    address: Address,
    topics: Vec<B256>,
    decoder: Decoder,
    logs: Arc<dyn LogReader>,
    ledger: Box<dyn EventLedger>,
}

impl AaveEventProcessor {
    #[must_use]
    pub fn new(
        chain_id: ChainId,
        contract: Contract,
        address: Address,
        logs: Arc<dyn LogReader>,
        ledger: Box<dyn EventLedger>,
    ) -> Self {
        Self {
            // Names the contract, so a retry reason says which stream stalled once there are two.
            name: format!("aave-{contract}({})", &format!("{address:#x}")[..10]),
            chain_id,
            address,
            topics: contract.topics(),
            decoder: Decoder::new(chain_id, contract, address),
            logs,
            ledger,
        }
    }
}

#[async_trait]
impl BlockProcessor for AaveEventProcessor {
    fn name(&self) -> &str {
        &self.name
    }

    async fn on_block_range(
        &self,
        from: u64,
        to: u64,
        cancel: &CancellationToken,
    ) -> Result<Outcome, Failure> {
        let filter = LogFilter {
            addresses: vec![self.address],
            topic0: self.topics.clone(),
            from_block: from,
            to_block: to,
        };
        let logs = match self.logs.logs(&filter).await {
            Ok(logs) => logs,
            Err(refused @ ChainError::RangeTooLarge { .. }) => {
                return Ok(Outcome::Retry {
                    reason: refused.to_string(),
                    narrow_range: true,
                });
            }
            Err(unavailable) => return Err(unavailable.into()),
        };
        let events = match self.decoder.decode(&logs) {
            Ok(events) => events,
            // The provider was asked for exactly these topics, so the ABI or the filter is wrong
            // and no retry fixes either.
            Err(undecodable) => {
                return Ok(Outcome::Failed {
                    reason: undecodable.to_string(),
                });
            }
        };

        // The cursor has not moved, so the range is dispatched again on the next start.
        if cancel.is_cancelled() {
            return Ok(Outcome::Retry {
                reason: "shutting down before the write".to_owned(),
                narrow_range: false,
            });
        }

        self.ledger.revert(self.chain_id, from, to).await?;
        self.ledger.append(&events).await?;

        if !events.is_empty() {
            tracing::info!(processor = %self.name, from, to, events = events.len(), "stored events");
        }
        Ok(Outcome::Ok)
    }

    async fn on_reorg(
        &self,
        from: u64,
        to: u64,
        _cancel: &CancellationToken,
    ) -> Result<Outcome, Failure> {
        tracing::warn!(processor = %self.name, from, to, "reorg: retracting blocks");
        self.ledger.revert(self.chain_id, from, to).await?;
        Ok(Outcome::Ok)
    }
}
