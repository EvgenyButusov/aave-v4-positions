//! The Aave V4 Spoke and Hub events this service follows, decoded into the shape their two
//! append-only ledgers keep, and the ledgers themselves.

mod contract;
mod decode;
#[cfg(test)]
mod fixtures;
mod ledger;

pub use contract::Contract;
pub use decode::{DecodedEvent, Decoder, Reason, Undecodable};
pub use ledger::{ClickHouseEventLedger, Error};
