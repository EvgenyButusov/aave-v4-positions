//! The port in a map, held to the same conformance cases as Postgres so the processor's suite
//! drives something that behaves like the store.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use alloy_primitives::Address;
use units::ChainId;

use crate::conformance::Harness;
use crate::{Error, TokenLabel, TokenMetadataRow, TokenMetadataStore};

#[derive(Default)]
pub(crate) struct InMemoryStore {
    rows: Mutex<HashMap<(ChainId, Address), TokenMetadataRow>>,
    pub(crate) label_reads: AtomicUsize,
    /// Never answers `labels` while set.
    pub(crate) stalled: AtomicBool,
}

impl InMemoryStore {
    pub(crate) fn row(&self, chain_id: ChainId, token: Address) -> Option<TokenMetadataRow> {
        self.rows.lock().unwrap().get(&(chain_id, token)).cloned()
    }

    pub(crate) fn len(&self) -> usize {
        self.rows.lock().unwrap().len()
    }
}

#[async_trait::async_trait]
impl TokenMetadataStore for InMemoryStore {
    async fn labels(&self, chain_id: ChainId) -> Result<HashMap<Address, TokenLabel>, Error> {
        self.label_reads.fetch_add(1, Ordering::SeqCst);
        if self.stalled.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }

        Ok(self
            .rows
            .lock()
            .unwrap()
            .values()
            .filter(|row| row.chain_id == chain_id)
            .map(|row| {
                (
                    row.token,
                    TokenLabel {
                        symbol: row.symbol.clone(),
                        name: row.name.clone(),
                    },
                )
            })
            .collect())
    }

    async fn put(&self, rows: &[TokenMetadataRow]) -> Result<(), Error> {
        let mut stored = self.rows.lock().unwrap();
        for row in rows {
            stored.insert((row.chain_id, row.token), row.clone());
        }
        Ok(())
    }
}

impl Harness for InMemoryStore {
    type Store = Self;

    async fn fresh(_case: &str) -> Self {
        Self::default()
    }

    fn store(&self) -> &Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::InMemoryStore;
    use crate::conformance;

    macro_rules! conformance {
        ($($case:ident),* $(,)?) => {
            $(
                #[tokio::test]
                async fn $case() {
                    conformance::$case::<InMemoryStore>().await;
                }
            )*
        };
    }

    conformance! {
        reads_every_label_on_the_chain,
        keeps_one_chain_apart_from_another_listing_the_same_address,
        keeps_a_token_that_answered_with_nothing,
        omits_a_token_with_no_row,
        replaces_a_token_read_again_rather_than_adding_to_it,
        writes_nothing_for_an_empty_batch,
    }
}
