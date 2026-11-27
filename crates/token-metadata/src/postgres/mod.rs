//! The Postgres implementation of [`TokenMetadataStore`](crate::TokenMetadataStore).
//!
//! The port's cases are in `conformance`; what is left here reads the columns the port cannot.

#[cfg(test)]
mod harness;
mod token_metadata_store;

pub use token_metadata_store::PostgresTokenMetadataStore;
