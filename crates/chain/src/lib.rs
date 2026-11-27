//! The indexer's chain ports over JSON-RPC: one ordered provider list behind all three, and what it
//! reports about who is answering.

mod config;
mod erc20;
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
