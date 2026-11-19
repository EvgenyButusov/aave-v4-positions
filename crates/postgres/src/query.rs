//! Statements that carry a span, because the driver opens none:
//! `tokio-postgres` speaks `log`, not `tracing`.

use tokio_postgres::Row;
use tokio_postgres::types::ToSql;

use crate::error::Error;
use crate::pool::{Pool, connection};

/// A pool whose statements are seen.
///
/// **The connection is taken per statement, never held.** `deadpool` checks
/// `is_closed()` when handing one out, so a held connection fails every request
/// after a server restart while readiness, asking for a fresh one, still passes.
pub struct TracedPool(Pool);

impl TracedPool {
    #[must_use]
    pub fn new(pool: Pool) -> Self {
        Self(pool)
    }

    /// `name` is `{operation} {target}`, given rather than parsed out of `sql`:
    /// parsing it produced a span called `SELECT its` from prose in a comment.
    ///
    /// # Errors
    ///
    /// No connection to be had, or one the server refused.
    pub async fn query(
        &self,
        name: &'static str,
        sql: &'static str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Vec<Row>, Error> {
        use tracing::Instrument as _;

        connection(&self.0)
            .await?
            .query(sql, params)
            .instrument(span(name, sql))
            .await
            .map_err(|source| Error::QueryFailed { source })
    }

    /// A write, and the rows it changed.
    ///
    /// Here rather than reached around through [`connection`], for the reason
    /// this type exists: an untraced write would be a gap nothing fails over.
    ///
    /// # Errors
    ///
    /// As [`query`](Self::query).
    pub async fn execute(
        &self,
        name: &'static str,
        sql: &'static str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<u64, Error> {
        use tracing::Instrument as _;

        connection(&self.0)
            .await?
            .execute(sql, params)
            .instrument(span(name, sql))
            .await
            .map_err(|source| Error::QueryFailed { source })
    }

    /// At most one row.
    ///
    /// # Errors
    ///
    /// As [`query`](Self::query).
    pub async fn query_opt(
        &self,
        name: &'static str,
        sql: &'static str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Option<Row>, Error> {
        use tracing::Instrument as _;

        connection(&self.0)
            .await?
            .query_opt(sql, params)
            .instrument(span(name, sql))
            .await
            .map_err(|source| Error::QueryFailed { source })
    }
}

fn span(name: &'static str, sql: &'static str) -> tracing::Span {
    tracing::info_span!(
        "postgres.query",
        otel.name = name,
        otel.kind = "client",
        db.system.name = "postgresql",
        db.query.text = sql,
    )
}

#[cfg(test)]
mod tests {
    use test_tracing::{Captured, Lines};
    use tracing::instrument::WithSubscriber as _;
    use tracing_subscriber::fmt::format::FmtSpan;

    use super::*;
    use crate::build_pool;
    use crate::pool::Limits;

    /// A statement every server has the answer to, so this needs no schema.
    const TABLES: &str =
        "SELECT schemaname FROM pg_catalog.pg_tables WHERE schemaname = $1 LIMIT 1";

    /// A pool and somewhere to read its spans back from, which both cases want.
    fn watched() -> (TracedPool, Lines, impl tracing::Subscriber) {
        test_tracing::interesting();

        let url = std::env::var("POSTGRES_URL").unwrap_or_else(|_| {
            "postgres://postgres@localhost:5432/postgres?sslmode=disable".to_owned()
        });
        let pool = build_pool(&url, Limits::default()).unwrap();
        let (captured, lines) = Captured::new();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(captured)
            .with_ansi(false)
            .with_span_events(FmtSpan::CLOSE)
            .finish();

        (TracedPool::new(pool), lines, subscriber)
    }

    #[tokio::test]
    async fn a_read_is_one_client_span_naming_what_it_read() {
        let (client, lines, subscriber) = watched();

        client
            .query("SELECT pg_tables", TABLES, &[&"public"])
            .with_subscriber(subscriber)
            .await
            .unwrap();

        let written = lines.read();

        assert!(
            written.contains(r#"otel.name="SELECT pg_tables""#),
            "{written}"
        );
        assert!(written.contains(r#"otel.kind="client""#), "{written}");
        assert!(
            written.contains(r#"db.system.name="postgresql""#),
            "{written}"
        );
        assert!(written.contains("WHERE schemaname = $1"), "{written}");
        assert!(
            !written.contains("public"),
            "a bound value reached the span"
        );
    }

    /// Three rows and a bound value this crate owns no table to hold.
    ///
    /// The count comes off the server's `CommandComplete` tag, which a `SELECT`
    /// carries exactly as a write does — measured: this returns 3 where `SET`
    /// returns 0 — so the one statement kind needing no schema is what proves
    /// the count is reported rather than invented.
    const ROWS: &str = "SELECT $1::text FROM generate_series(1, 3)";

    #[tokio::test]
    async fn a_write_is_one_client_span_and_the_count_the_server_reported() {
        let (client, lines, subscriber) = watched();

        let changed = client
            .execute("INSERT three", ROWS, &[&"sunflower"])
            .with_subscriber(subscriber)
            .await
            .unwrap();

        let written = lines.read();

        assert_eq!(changed, 3);
        assert!(written.contains(r#"otel.name="INSERT three""#), "{written}");
        assert!(written.contains(r#"otel.kind="client""#), "{written}");
        assert!(
            written.contains(r#"db.system.name="postgresql""#),
            "{written}"
        );
        assert!(written.contains("generate_series(1, 3)"), "{written}");
        assert!(
            !written.contains("sunflower"),
            "a bound value reached the span"
        );
    }
}
