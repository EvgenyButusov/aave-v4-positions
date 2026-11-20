#[cfg(test)]
mod tests;

use alloy_primitives::{Address, address};
use units::ChainId;

/// A Spoke, the oracle that prices its reserves, and the Hub its reserves draw on. A second Spoke is
/// a second value, not an edit to this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deployment {
    pub chain_id: ChainId,
    pub spoke: Address,
    /// Indexed by the Spoke's own `reserveId`, so it belongs to this Spoke and to no other.
    pub spoke_oracle: Address,
    pub hub: Address,
    /// The earliest of the contracts' first logs. The Hub's comes first, and its first `AddAsset` is
    /// the only source of an asset's underlying and decimals: starting later misses it silently.
    pub first_block: u64,
}

pub const MAIN_SPOKE_ON_ETHEREUM: Deployment = Deployment {
    chain_id: ChainId::known(1),
    spoke: address!("0x94e7A5dCbE816e498b89aB752661904E2F56c485"),
    spoke_oracle: address!("0x99B2B6CEa9C3D2fd8F4d90f86741C44B212a6127"),
    hub: address!("0xCca852Bc40e560adC3b1Cc58CA5b55638ce826c9"),
    first_block: 24_720_891,
};
