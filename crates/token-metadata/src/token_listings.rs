use alloy_primitives::Address;
use units::ChainId;

use crate::Error;

/// Which ERC-20s the Hub has listed: the chain-side half of the gap enrichment fills.
#[async_trait::async_trait]
pub trait TokenListings: Send + Sync {
    /// Each listed token once, including one whose listing a reorg retracted: reading a token
    /// nobody lists costs one wasted call, while missing one leaves it unlabelled for good.
    ///
    /// # Errors
    ///
    /// [`Error::ListingsReadFailed`] when the listings cannot be read.
    async fn all(&self, chain_id: ChainId) -> Result<Vec<Address>, Error>;
}
