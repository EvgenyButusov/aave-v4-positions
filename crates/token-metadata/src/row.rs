use alloy_primitives::Address;
use units::ChainId;

/// What one read of a token found, as `put` stores it.
///
/// `token_decimals` is the token's own, not what amounts are scaled by: the Hub's `AddAsset`
/// carries that, so the two can disagree and only the Hub's values a position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenMetadataRow {
    pub chain_id: ChainId,
    pub token: Address,
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub token_decimals: Option<u8>,
    pub fetched_at_block: u64,
}
