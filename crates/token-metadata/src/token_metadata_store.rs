use std::collections::HashMap;

use alloy_primitives::Address;

use units::ChainId;

use crate::{Error, TokenLabel, TokenMetadataRow};

/// What tokens call themselves. No pagination: a Hub lists seventeen assets on mainnet.
#[async_trait::async_trait]
pub trait TokenMetadataStore: Send + Sync {
    /// Every label on this chain, keyed by token.
    ///
    /// The whole dimension, not the tokens on a page: taking a page's addresses would force this
    /// to run after the ClickHouse query, where keyed by chain alone it runs beside it.
    ///
    /// A token with no row is absent from the map rather than present with nulls, so a caller can
    /// tell "never asked" from "asked, and has none".
    ///
    /// # Errors
    ///
    /// [`Error::LabelsReadFailed`] when the query cannot be taken, [`Error::Malformed`] when a row
    /// cannot be read.
    async fn labels(&self, chain_id: ChainId) -> Result<HashMap<Address, TokenLabel>, Error>;

    /// Stores each row whole, replacing what an earlier read of the same token stored. All or none.
    ///
    /// # Errors
    ///
    /// [`Error::WriteFailed`], having written nothing.
    async fn put(&self, rows: &[TokenMetadataRow]) -> Result<(), Error>;
}
