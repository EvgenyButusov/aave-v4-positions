//! Traces, metrics and logs over OTLP, and the subscriber they pass through.
//!
//! stdout keeps its one JSON object per line; the OTLP copy is additive.
//! [`Settings::read`] takes the binary's map; nothing here reads `std::env`.

mod context;
mod panics;
mod provider;
mod settings;
mod subscriber;

pub use context::{continue_from, trace_id};
pub use settings::{Endpoints, Sampling, Settings};
pub use subscriber::{Error, SCOPE, Telemetry, init};
