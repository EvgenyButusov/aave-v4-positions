//! The ClickHouse adapters over the Aave ledgers, and over the fold's tables where another crate's
//! port asks a question only the fold can answer.

#[cfg(test)]
mod harness;
mod ledger;
mod token_listings;

pub use ledger::ClickHouseEventLedger;
pub use token_listings::ClickHouseTokenListings;
