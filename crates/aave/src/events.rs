//! The Aave V4 Spoke and Hub events this service follows, decoded into the shape their two
//! append-only ledgers keep, the port those ledgers are written through, and the processor that
//! reads a range into one.

mod contract;
mod decode;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;
mod ledger;
mod processor;

pub use contract::Contract;
pub use decode::{DecodedEvent, Decoder, Reason, Undecodable};
pub use ledger::{Error, EventLedger};
pub use processor::AaveEventProcessor;
