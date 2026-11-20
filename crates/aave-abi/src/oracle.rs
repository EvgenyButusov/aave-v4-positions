//! `sol!` over `abi/IAaveOracleV4.json`: `build.rs` writes a repaired copy and the call into `OUT_DIR`.

include!(concat!(env!("OUT_DIR"), "/oracle.rs"));

/// The vendored ABI, as `@aave-dao/aave-address-book` publishes it.
pub const JSON: &str = include_str!("../abi/IAaveOracleV4.json");
