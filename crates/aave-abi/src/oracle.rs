//! `abi/IAaveOracleV4.json` twice over: the `sol!` types `build.rs` generates, and the raw JSON.

include!(concat!(env!("OUT_DIR"), "/oracle_bindings.rs"));

/// The vendored ABI as published, for code that reads it at runtime.
pub const ABI_JSON: &str = include_str!("../abi/IAaveOracleV4.json");
