//! The Aave V4 contracts this service follows: their ABIs as `@aave-dao/aave-address-book`
//! publishes them, and where the Main Spoke's deployment lives.
//!
//! One module per ABI, because each declares its own copy of the structs they share.

mod deployment;
pub mod hub;
pub mod oracle;
pub mod spoke;
#[cfg(test)]
mod tests;

pub use deployment::{Deployment, MAIN_SPOKE_ON_ETHEREUM};
