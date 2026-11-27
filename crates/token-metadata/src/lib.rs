//! What an ERC-20 calls itself, read from the token rather than folded: no Aave event carries a
//! symbol (§12.5). Why it lives in Postgres is measured in `010_token_metadata.sql`.

mod error;
mod label;
mod postgres;
mod row;
mod token_listings;
mod token_metadata_store;

#[cfg(test)]
mod conformance;

pub use error::Error;
pub use label::TokenLabel;
pub use postgres::PostgresTokenMetadataStore;
pub use row::TokenMetadataRow;
pub use token_listings::TokenListings;
pub use token_metadata_store::TokenMetadataStore;
