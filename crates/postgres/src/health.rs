use crate::error::Error;
use crate::pool::{Limits, Pool, build_pool, connection};

/// The only pool [`ping`] takes, so readiness cannot be handed the pool that
/// does the work and queue behind it.
#[derive(Debug, Clone)]
pub struct ProbePool(Pool);

/// One connection, whatever `limits.max_size` says. Not `Timeouts::wait_millis(0)`
/// on a shared pool: deadpool turns a zero wait into a non-blocking `try_acquire`
/// and would report a busy pool as a down database.
///
/// # Errors
///
/// Whatever [`build_pool`] refuses the URL for.
pub fn probe_pool(url: &str, limits: Limits) -> Result<ProbePool, Error> {
    build_pool(
        url,
        Limits {
            max_size: 1,
            ..limits
        },
    )
    .map(ProbePool)
}

/// Reports Postgres able to serve a query.
///
/// Deliberately through the pool rather than around it: what a caller needs to
/// know is whether the next *query* can be served, and a pool with no connection
/// to give is as unserviceable as a server that is down.
///
/// Schema integrity belongs to the migration gate, not to a recurring probe.
///
/// # Errors
///
/// [`Error::ConnectFailed`] if no connection can be had, [`Error::PingFailed`]
/// if the server refuses the query.
pub async fn ping(ProbePool(pool): &ProbePool) -> Result<(), Error> {
    connection(pool)
        .await?
        .simple_query("SELECT 1")
        .await
        .map(drop)
        .map_err(|source| Error::PingFailed { source })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(url: &str) -> ProbePool {
        probe_pool(url, Limits::default()).expect("a parseable URL")
    }

    fn served() -> ProbePool {
        pool(&std::env::var("POSTGRES_URL").unwrap_or_else(|_| {
            "postgres://postgres@localhost:5432/postgres?sslmode=disable".to_owned()
        }))
    }

    #[tokio::test]
    async fn answers_when_the_server_can_serve_a_query() {
        ping(&served()).await.expect("reachable");

        let refused = ping(&pool("postgres://postgres@127.0.0.1:1/nothing"))
            .await
            .expect_err("port 1 is reserved");

        assert_eq!(refused.to_string(), "could not connect to Postgres");
    }

    #[test]
    fn a_probe_pool_holds_one_connection_whatever_the_limits_say() {
        let limits = Limits {
            max_size: 8,
            ..Limits::default()
        };
        let ProbePool(pool) =
            probe_pool("postgres://postgres@127.0.0.1:1/nothing", limits).unwrap();

        assert_eq!(pool.status().max_size, 1);
    }
}
