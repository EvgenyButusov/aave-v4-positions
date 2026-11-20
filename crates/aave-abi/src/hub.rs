//! `sol!` over `abi/IHubV4.json`: `build.rs` writes a repaired copy and the call into `OUT_DIR`.

include!(concat!(env!("OUT_DIR"), "/hub.rs"));

/// The vendored ABI, as `@aave-dao/aave-address-book` publishes it.
pub const JSON: &str = include_str!("../abi/IHubV4.json");
