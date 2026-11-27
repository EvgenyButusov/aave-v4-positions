use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard, PoisonError};

use alloy_primitives::Address;

/// Tokens the Hub has just listed, handed over by the processor that wrote the `AddAsset` and
/// waiting to be read. A set, so a token listed twice or a range dispatched again is one read.
///
/// In memory only: a restart loses it, and the full check every start owes recovers what it held.
#[derive(Debug, Default)]
pub struct PendingTokens(Mutex<HashSet<Address>>);

impl PendingTokens {
    pub fn add(&self, tokens: &[Address]) {
        self.tokens().extend(tokens);
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tokens().is_empty()
    }

    /// Everything, leaving it empty. A token the run then fails to read is recovered by the full
    /// check its back-off owes, not by staying here.
    #[must_use]
    pub fn drain(&self) -> Vec<Address> {
        self.tokens().drain().collect()
    }

    fn tokens(&self) -> MutexGuard<'_, HashSet<Address>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::address;

    use super::PendingTokens;

    const USDC: alloy_primitives::Address = address!("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");
    const WETH: alloy_primitives::Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");

    #[test]
    fn is_empty_until_ingestion_puts_something_in_it() {
        assert!(PendingTokens::default().is_empty());
    }

    #[test]
    fn hands_back_what_was_added_then_forgets_it() {
        let pending = PendingTokens::default();
        pending.add(&[USDC, WETH]);

        let mut drained = pending.drain();
        drained.sort();

        assert_eq!(drained, [USDC, WETH]);
        assert!(pending.is_empty());
        assert!(pending.drain().is_empty());
    }

    #[test]
    fn accumulates_across_ranges_until_something_drains_it() {
        let pending = PendingTokens::default();
        pending.add(&[USDC]);
        pending.add(&[WETH]);

        let mut drained = pending.drain();
        drained.sort();

        assert_eq!(drained, [USDC, WETH]);
    }

    #[test]
    fn keeps_one_entry_for_a_token_added_twice() {
        let pending = PendingTokens::default();
        pending.add(&[USDC]);
        pending.add(&[USDC]);

        assert_eq!(pending.drain(), [USDC]);
    }
}
