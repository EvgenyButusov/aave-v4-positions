//! `abi/ISpokeV4.json` twice over: the `sol!` types `build.rs` generates, and the raw JSON.

#![expect(
    clippy::too_many_arguments,
    reason = "`sol!` gives each event a constructor over every field, and `LiquidationCall` has eleven"
)]

include!(concat!(env!("OUT_DIR"), "/spoke_bindings.rs"));

/// The vendored ABI as published, for code that reads it at runtime.
pub const ABI_JSON: &str = include_str!("../abi/ISpokeV4.json");
