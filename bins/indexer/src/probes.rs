//! What a deployment asks of this process. The checks are named here, and that is the whole
//! registry.

use std::sync::Arc;

use axum::Router;
use clickhouse_client::clickhouse::Client;
use ops::{ShutdownFlag, Uptime};
use postgres::ProbePool;

use crate::stall::Stall;

pub(crate) struct Dependencies {
    stall: Stall,
    clickhouse: Client,
    postgres: ProbePool,
}

pub(crate) fn dependencies(stall: Stall, clickhouse: Client, postgres: ProbePool) -> Dependencies {
    Dependencies {
        stall,
        clickhouse,
        postgres,
    }
}

pub(crate) fn routes(uptime: Uptime, shutdown: ShutdownFlag, dependencies: Dependencies) -> Router {
    let dependencies = Arc::new(dependencies);

    ops::probe_router(uptime, shutdown, move || {
        let dependencies = Arc::clone(&dependencies);
        async move {
            let indexer = dependencies.stall.check();
            let (clickhouse, postgres) = tokio::join!(
                ops::check(
                    "clickhouse",
                    clickhouse_client::ping(&dependencies.clickhouse)
                ),
                ops::check("postgres", ::postgres::ping(&dependencies.postgres)),
            );
            vec![indexer, clickhouse, postgres]
        }
    })
}

#[cfg(test)]
mod tests {
    //! Against the real servers, because the names, the order and the two pings behind them are
    //! all this file adds: `ops` proves the report and the drain on its own.

    use std::time::Duration;

    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use clickhouse_client::TransportSecurity;
    use env::Env;
    use indexing::IndexerStatus;
    use tower::ServiceExt as _;

    use super::*;

    /// What `config` would read from this process's environment, so a case asks the database a
    /// deployment would have. CI's server speaks no TLS and sets no URL.
    fn dependencies_from_the_environment(status: Arc<IndexerStatus>) -> Dependencies {
        let local = "postgres://postgres@localhost:5432/postgres?sslmode=disable";
        let env: Env = [("POSTGRES_URL".to_owned(), local.to_owned())]
            .into_iter()
            .chain(std::env::vars())
            .collect();
        let clickhouse = clickhouse_client::build_client(clickhouse_client::Config {
            transport_security: TransportSecurity::AllowInsecureHttp,
            ..clickhouse_client::Config::read(&env).unwrap()
        })
        .unwrap();

        dependencies(
            Stall::new(status, Duration::from_secs(300)),
            clickhouse,
            postgres::probe_pool(
                &postgres::read_url(&env).unwrap(),
                postgres::Limits::default(),
            )
            .unwrap(),
        )
    }

    async fn ready(status: Arc<IndexerStatus>) -> (StatusCode, String) {
        let router = routes(
            Uptime::now(),
            ShutdownFlag::new(),
            dependencies_from_the_environment(status),
        );
        let request = Request::get("/health/ready").body(Body::empty()).unwrap();
        let response = router.oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn a_starting_indexer_over_both_databases_is_ready() {
        let (status, body) = ready(Arc::new(IndexerStatus::new())).await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body,
            r#"{"status":"ok","checks":[{"name":"indexer","status":"up"},{"name":"clickhouse","status":"up"},{"name":"postgres","status":"up"}]}"#
        );
    }

    #[tokio::test]
    async fn a_stopped_loop_fails_readiness_with_both_databases_up() {
        let status = Arc::new(IndexerStatus::new());
        status.failed("chain 5 is not chain 1".to_owned());

        let (status, body) = ready(status).await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            body,
            r#"{"status":"degraded","checks":[{"name":"indexer","status":"down","error":"indexing stopped: chain 5 is not chain 1"},{"name":"clickhouse","status":"up"},{"name":"postgres","status":"up"}]}"#
        );
    }
}
