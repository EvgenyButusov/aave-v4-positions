//! The ClickHouse adapters behind the ports `aave` defines.

mod ledger;

pub use ledger::ClickHouseEventLedger;
