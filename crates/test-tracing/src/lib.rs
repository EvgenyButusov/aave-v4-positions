//! Only ever a dev-dependency: what a scoped subscriber saw, read back, for suites whose crates
//! cannot depend on one another.

mod captured;
mod interest;

pub use captured::{Captured, Lines};
pub use interest::interesting;
