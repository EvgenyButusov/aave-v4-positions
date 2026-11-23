//! The Aave V4 contracts this service follows: their ABIs as `@aave-dao/aave-address-book`
//! publishes them, where the Main Spoke's deployment lives, and their events and ledgers.

pub mod abi;
mod deployment;
pub mod events;

pub use deployment::{Deployment, MAIN_SPOKE_ON_ETHEREUM};
