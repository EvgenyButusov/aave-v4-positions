//! The indexer's chain ports over JSON-RPC: one ordered provider list behind both, and what it
//! reports about who is answering.

mod config;
mod error;
#[cfg(test)]
mod harness;
mod http;
mod json_rpc;
mod providers;
mod refusal;
#[cfg(test)]
mod scripted;

pub use config::Config;
pub use error::Error;
pub use json_rpc::JsonRpc;
