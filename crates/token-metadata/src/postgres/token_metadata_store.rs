use std::collections::HashMap;

use alloy_primitives::Address;
use postgres::{Bigint, Pool, TracedPool};
use tokio_postgres::Row;
use units::ChainId;

use crate::{Error, TokenLabel, TokenMetadataRow, TokenMetadataStore};

const LABELS: &str = "\
    SELECT token, symbol, name \
    FROM token_metadata \
    WHERE chain_id = $1";

/// One array per column, so a batch is one statement and one transaction however many rows it
/// carries: the shape the sqlx FAQ gives for a bulk insert. `fetched_at` is the server's clock.
const PUT: &str = "\
    INSERT INTO token_metadata \
        (chain_id, token, symbol, name, token_decimals, fetched_at_block) \
    SELECT * FROM UNNEST($1::bigint[], $2::text[], $3::text[], $4::text[], $5::smallint[], $6::bigint[]) \
    ON CONFLICT (chain_id, token) DO UPDATE \
       SET symbol           = EXCLUDED.symbol, \
           name             = EXCLUDED.name, \
           token_decimals   = EXCLUDED.token_decimals, \
           fetched_at       = now(), \
           fetched_at_block = EXCLUDED.fetched_at_block";

pub struct PostgresTokenMetadataStore {
    db: TracedPool,
}

impl PostgresTokenMetadataStore {
    #[must_use]
    pub fn new(pool: Pool) -> Self {
        Self {
            db: TracedPool::new(pool),
        }
    }
}

#[async_trait::async_trait]
impl TokenMetadataStore for PostgresTokenMetadataStore {
    async fn labels(&self, chain_id: ChainId) -> Result<HashMap<Address, TokenLabel>, Error> {
        let rows = self
            .db
            .query("SELECT token_metadata", LABELS, &[&Bigint(chain_id.into())])
            .await
            .map_err(Error::LabelsReadFailed)?;

        rows.iter().map(label).collect()
    }

    async fn put(&self, rows: &[TokenMetadataRow]) -> Result<(), Error> {
        if rows.is_empty() {
            return Ok(());
        }

        let chain_ids: Vec<Bigint> = rows.iter().map(|row| Bigint(row.chain_id.into())).collect();
        // Lower-case, which the column's `CHECK` demands and `Address`'s checksummed `Display` is not.
        let tokens: Vec<String> = rows.iter().map(|row| format!("{:#x}", row.token)).collect();
        let symbols: Vec<Option<&str>> = rows.iter().map(|row| row.symbol.as_deref()).collect();
        let names: Vec<Option<&str>> = rows.iter().map(|row| row.name.as_deref()).collect();
        let decimals: Vec<Option<i16>> = rows
            .iter()
            .map(|row| row.token_decimals.map(i16::from))
            .collect();
        let blocks: Vec<Bigint> = rows
            .iter()
            .map(|row| Bigint(row.fetched_at_block))
            .collect();

        self.db
            .execute(
                "INSERT token_metadata",
                PUT,
                &[&chain_ids, &tokens, &symbols, &names, &decimals, &blocks],
            )
            .await
            .map(drop)
            .map_err(Error::WriteFailed)
    }
}

fn label(row: &Row) -> Result<(Address, TokenLabel), Error> {
    let token: &str = row.get("token");

    let token = token.parse().map_err(|_| Error::Malformed {
        column: "token",
        expected: "20-byte address",
        value: token.to_owned(),
    })?;

    Ok((
        token,
        TokenLabel {
            symbol: row.get("symbol"),
            name: row.get("name"),
        },
    ))
}

#[cfg(test)]
mod tests {
    use alloy_primitives::Address;

    use crate::conformance::{self, CHAIN_ID, Harness as _, USDC, WETH, labelled};
    use crate::postgres::harness::PostgresHarness;
    use crate::{Error, TokenMetadataRow, TokenMetadataStore as _};

    macro_rules! conformance {
        ($($case:ident),* $(,)?) => {
            $(
                #[tokio::test]
                async fn $case() {
                    conformance::$case::<PostgresHarness>().await;
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

    #[tokio::test]
    async fn writes_every_field_to_its_own_column() {
        let harness = PostgresHarness::fresh("writes_every_field").await;

        harness
            .store()
            .put(&[labelled(CHAIN_ID, USDC, "USDC")])
            .await
            .unwrap();

        let [row] = harness.stored().await.try_into().unwrap();
        assert_eq!(row.get::<_, i64>("chain_id"), 1);
        assert_eq!(row.get::<_, Option<&str>>("symbol"), Some("USDC"));
        assert_eq!(row.get::<_, Option<&str>>("name"), Some("USDC Token"));
        assert_eq!(row.get::<_, Option<i16>>("token_decimals"), Some(6));
        assert_eq!(row.get::<_, i64>("fetched_at_block"), 25_652_782);
    }

    #[tokio::test]
    async fn stores_the_address_in_the_case_the_fold_joins_on() {
        let harness = PostgresHarness::fresh("stores_the_address_lower_cased").await;
        let checksummed: Address = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
            .parse()
            .unwrap();

        harness
            .store()
            .put(&[labelled(CHAIN_ID, checksummed, "USDC")])
            .await
            .unwrap();

        let [row] = harness.stored().await.try_into().unwrap();
        assert_eq!(
            row.get::<_, &str>("token"),
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
        );
    }

    #[tokio::test]
    async fn replaces_every_column_and_restamps_the_read() {
        let harness = PostgresHarness::fresh("replaces_every_column").await;
        let store = harness.store();
        store
            .put(&[labelled(CHAIN_ID, USDC, "USDC")])
            .await
            .unwrap();
        let [before] = harness.stored().await.try_into().unwrap();

        store
            .put(&[TokenMetadataRow {
                symbol: None,
                name: Some("Bridged USDC".to_owned()),
                token_decimals: Some(18),
                fetched_at_block: 25_700_000,
                ..labelled(CHAIN_ID, USDC, "USDC")
            }])
            .await
            .unwrap();

        let [after] = harness.stored().await.try_into().unwrap();
        assert_eq!(after.get::<_, Option<&str>>("symbol"), None);
        assert_eq!(after.get::<_, Option<&str>>("name"), Some("Bridged USDC"));
        assert_eq!(after.get::<_, Option<i16>>("token_decimals"), Some(18));
        assert_eq!(after.get::<_, i64>("fetched_at_block"), 25_700_000);
        assert!(
            after.get::<_, i64>("fetched_at_micros") > before.get::<_, i64>("fetched_at_micros")
        );
    }

    #[tokio::test]
    async fn refuses_a_batch_whole_rather_than_writing_part_of_it() {
        let harness = PostgresHarness::fresh("refuses_a_batch_whole").await;
        let past_bigint = TokenMetadataRow {
            fetched_at_block: u64::MAX,
            ..labelled(CHAIN_ID, WETH, "WETH")
        };

        let refused = harness
            .store()
            .put(&[labelled(CHAIN_ID, USDC, "USDC"), past_bigint])
            .await;

        assert!(matches!(refused, Err(Error::WriteFailed(_))), "{refused:?}");
        assert!(harness.stored().await.is_empty());
    }
}
