#![expect(
    clippy::too_many_arguments,
    reason = "`sol!` gives each event a constructor over every field, and `LiquidationCall` has eleven"
)]

include!(concat!(env!("OUT_DIR"), "/spoke.rs"));
