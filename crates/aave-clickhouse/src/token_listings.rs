#[cfg(test)]
mod tests;

use alloy_primitives::Address;
use async_trait::async_trait;
use clickhouse_client::clickhouse::Client;
use token_metadata::{Error, TokenListings};
use units::ChainId;

/// Answered from `hub_asset_state`'s `listed_tokens` projection, which the optimizer picks for this
/// exact shape; reworded, the same question scans every `UpdateAsset` row instead.
///
/// No `sign` filter: a retraction is a second row, and the `+1` it cancels stays until a merge.
const LISTED: &str = "SELECT DISTINCT underlying \
                      FROM hub_asset_state \
                      WHERE chain_id = {chainId:UInt64} AND underlying IS NOT NULL";

#[derive(Clone)]
pub struct ClickHouseTokenListings {
    client: Client,
}

impl ClickHouseTokenListings {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl TokenListings for ClickHouseTokenListings {
    async fn all(&self, chain_id: ChainId) -> Result<Vec<Address>, Error> {
        let listed = self
            .client
            .query(LISTED)
            .param("chainId", chain_id.get())
            .fetch_all::<Option<String>>()
            .await
            .map_err(|source| Error::ListingsReadFailed(source.into()))?;

        listed
            .into_iter()
            .flatten()
            // What the fold writes for an `AddAsset` whose body has no `underlying`.
            .filter(|token| !token.is_empty())
            .map(|token| {
                token.parse().map_err(|_| Error::Malformed {
                    column: "underlying",
                    expected: "20-byte address",
                    value: token,
                })
            })
            .collect()
    }
}
