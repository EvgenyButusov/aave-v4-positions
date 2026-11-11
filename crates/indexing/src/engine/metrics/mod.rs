//! What the loop reports about itself: what it records, and what is read off its status.

mod gauges;
mod instruments;

pub use instruments::Instruments;
pub(crate) use instruments::Phase;
