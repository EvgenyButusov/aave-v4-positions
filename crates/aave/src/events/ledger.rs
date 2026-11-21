use async_trait::async_trait;
use units::ChainId;

use crate::events::{Contract, DecodedEvent};

/// One contract's ledger, written by appending only.
///
/// **[`revert`](Self::revert) the range, then [`append`](Self::append), every time it is indexed.**
/// Dispatch is at least once and an append adds whatever it is given, so an append without the
/// revert before it leaves two live copies of the range.
#[async_trait]
pub trait EventLedger: Send + Sync {
    /// Removes every event of `chain_id` in `from..=to`, leaving any other chain's alone.
    ///
    /// # Errors
    ///
    /// [`Error::RevertFailed`] when the store refuses.
    async fn revert(&self, chain_id: ChainId, from: u64, to: u64) -> Result<(), Error>;

    /// # Errors
    ///
    /// [`Error::AppendFailed`] when the store refuses.
    async fn append(&self, events: &[DecodedEvent]) -> Result<(), Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the {contract} ledger could not retract blocks {from}..={to}")]
    RevertFailed {
        contract: Contract,
        from: u64,
        to: u64,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("the {contract} ledger could not append {events} events")]
    AppendFailed {
        contract: Contract,
        events: usize,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}
