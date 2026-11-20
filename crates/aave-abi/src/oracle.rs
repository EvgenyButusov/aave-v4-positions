//! `sol!` over `abi/IAaveOracleV4.json`: `build.rs` writes a repaired copy and the call into `OUT_DIR`.

include!(concat!(env!("OUT_DIR"), "/oracle.rs"));
