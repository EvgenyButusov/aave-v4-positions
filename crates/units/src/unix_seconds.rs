use std::fmt;

/// An instant, in whole seconds since the Unix epoch, as a block header reports
/// one. A type of its own so a height and an instant, both `u64`, cannot be
/// swapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnixSeconds(u64);

impl UnixSeconds {
    #[must_use]
    pub const fn new(seconds: u64) -> Self {
        Self(seconds)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<UnixSeconds> for u64 {
    fn from(instant: UnixSeconds) -> Self {
        instant.0
    }
}

impl fmt::Display for UnixSeconds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
