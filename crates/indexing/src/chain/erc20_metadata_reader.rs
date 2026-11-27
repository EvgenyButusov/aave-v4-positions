use alloy_primitives::Address;
use async_trait::async_trait;

/// What an ERC-20 says about itself. EIP-20 makes all three optional.
///
/// `decimals` is the token's own, not what amounts are scaled by: the Hub's `AddAsset` carries
/// that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenMetadata {
    pub symbol: Field<String>,
    pub name: Field<String>,
    pub decimals: Field<u8>,
}

/// One field, as the token answered it or did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field<T> {
    Value(T),

    /// The token answered with nothing usable, which asking again would not change.
    Absent(Absence),

    /// The call never reached the token, so this says nothing about it.
    Unreached(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Absence {
    #[error("the call reverted")]
    Reverted,

    /// No code at the address, or no such method.
    #[error("the call returned nothing")]
    NoData,

    #[error("the return could not be decoded")]
    Undecodable,

    #[error("the value does not fit its type")]
    OutOfRange,

    /// Nothing left once control characters and surrounding whitespace are gone.
    #[error("the label is blank")]
    Blank,
}

impl TokenMetadata {
    /// Whether every field is the token's final answer, so the read is worth keeping.
    #[must_use]
    pub fn answered(&self) -> bool {
        !(self.symbol.unreached() || self.name.unreached() || self.decimals.unreached())
    }
}

impl<T> Field<T> {
    #[must_use]
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Absent(_) | Self::Unreached(_) => None,
        }
    }

    fn unreached(&self) -> bool {
        matches!(self, Self::Unreached(_))
    }
}

/// Reading an ERC-20's metadata, which the loop does not do.
///
/// Its own port for the reason [`LogReader`](super::LogReader) gives. ERC-20 is an Ethereum
/// standard, so it sits beside the other chain ports; knowing *which* tokens to read would not.
#[async_trait]
pub trait Erc20MetadataReader: Send + Sync {
    /// Pinned to one block, so three reads across a failover agree on a height.
    async fn read(&self, token: Address, at_block: u64) -> TokenMetadata;
}
