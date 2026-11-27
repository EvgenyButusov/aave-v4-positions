//! A migrated schema, a store over it, and the table read back whole.

use postgres::{Pool, connection};
use test_db::Scratch;
use tokio_postgres::Row;

use super::PostgresTokenMetadataStore;
use crate::conformance::Harness;

pub(crate) struct PostgresHarness {
    pool: Pool,
    store: PostgresTokenMetadataStore,
}

impl Harness for PostgresHarness {
    type Store = PostgresTokenMetadataStore;

    async fn fresh(case: &str) -> Self {
        let pool = Scratch::migrated("rust_token_metadata", case)
            .await
            .pool()
            .clone();

        Self {
            store: PostgresTokenMetadataStore::new(pool.clone()),
            pool,
        }
    }

    fn store(&self) -> &Self::Store {
        &self.store
    }
}

impl PostgresHarness {
    /// Every column, including the two the port never reads back.
    pub(crate) async fn stored(&self) -> Vec<Row> {
        connection(&self.pool)
            .await
            .unwrap()
            .query(
                "SELECT chain_id, token, symbol, name, token_decimals, fetched_at_block, \
                        (extract(epoch FROM fetched_at) * 1000000)::bigint AS fetched_at_micros \
                 FROM token_metadata ORDER BY chain_id, token",
                &[],
            )
            .await
            .unwrap()
    }
}
