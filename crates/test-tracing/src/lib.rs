//! Only ever a dev-dependency. Five suites read back what a scoped subscriber saw, and each needed
//! the same workaround for one `tracing-core` bug; none of their crates is a home the others may
//! depend on.

mod interest;

pub use interest::interesting;
