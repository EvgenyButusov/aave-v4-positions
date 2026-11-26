//! Follows the chain into the two databases, and serves its probes.
//!
//! Boot order is config, telemetry, dependencies, loop, listener. Shutdown stops the listener first,
//! then the loop, which gets [`SHUTDOWN_DRAIN`] to finish its iteration.

mod config;
mod contracts;
mod pipeline;
mod probes;
mod stall;

use std::error::Error;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use aave_clickhouse::ClickHouseEventLedger;
use chain::JsonRpc;
use indexing::{Indexer, LogReader, SHUTDOWN_DRAIN};
use ops::{ShutdownFlag, Uptime};

use config::Config;
use stall::Stall;

#[tokio::main]
async fn main() -> ExitCode {
    // Before anything else, so `uptime_seconds` counts from the process.
    let uptime = Uptime::now();

    match run(uptime).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // On stderr: the likely failure is the configuration that would have said how to log.
            eprintln!("indexer: {error}");
            let mut cause = error.source();
            while let Some(next) = cause {
                eprintln!("  caused by: {next}");
                cause = next.source();
            }
            ExitCode::FAILURE
        }
    }
}

async fn run(uptime: Uptime) -> Result<(), Box<dyn Error>> {
    let config = Config::from_env()?;
    let telemetry = telemetry::init(config.telemetry.clone())?;

    // Hosts, never URLs: a provider's URL carries its key.
    tracing::info!(
        chain_id = config.chain.chain_id.get(),
        start_block = config.chain.start_block,
        max_range_size = config.chain.max_range_size,
        poll_interval = ?config.chain.poll_interval,
        finality_depth = config.finality_depth,
        providers = %config.rpc.hosts().join(", "),
        rpc_timeout = ?config.rpc.timeout,
        spoke = %config.spoke,
        hub = %config.hub,
        autostart = config.autostart,
        "indexer configured"
    );

    let clickhouse = clickhouse_client::build_client(config.clickhouse.clone())?;
    let meter = opentelemetry::global::meter(telemetry::SCOPE);
    let chain = Arc::new(JsonRpc::connect(&config.rpc, &meter)?);
    let logs: Arc<dyn LogReader> = Arc::clone(&chain) as _;
    let pipeline = pipeline::assemble(
        config.chain,
        config.finality_depth,
        chain,
        postgres::build_pool(&config.postgres_url, config.postgres_limits)?,
        contracts::processors(&config, &logs, |contract| {
            Box::new(ClickHouseEventLedger::new(clickhouse.clone(), contract))
        }),
        &meter,
    );
    let status = Arc::clone(&pipeline.status);
    let cancel = pipeline.cancel.clone();
    let mut indexer = Indexer::new(pipeline);
    let indexing = if config.autostart {
        Some(tokio::spawn(async move { indexer.run().await }))
    } else {
        tracing::info!("autostart disabled; not indexing");
        None
    };

    let probes = probes::dependencies(
        Stall::new(status, config.stall_threshold),
        clickhouse,
        // Not a clone of the loop's pool: a probe queued behind it reports a busy database as down.
        postgres::probe_pool(&config.postgres_url, config.postgres_limits)?,
    );

    let shutdown = ShutdownFlag::new();
    let router = probes::routes(uptime, shutdown.clone(), probes);

    let address = SocketAddr::new(config.host, config.port);
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, "indexer listening");

    axum::serve(listener, router)
        .with_graceful_shutdown(async move { shutdown.on_signal(config.grace).await })
        .await?;

    cancel.cancel();
    if let Some(indexing) = indexing
        && tokio::time::timeout(SHUTDOWN_DRAIN, indexing)
            .await
            .is_err()
    {
        tracing::warn!(drain = ?SHUTDOWN_DRAIN, "the loop did not finish its iteration in time");
    }

    tracing::info!("shutdown complete");

    // Last, and off a worker: the flush blocks on threads that post their final batch back
    // through this runtime.
    tokio::task::spawn_blocking(move || telemetry.shutdown()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    //! The one case through a socket: `axum::serve`, the listener and the shutdown future are this
    //! file's whole contribution.

    use std::time::Duration;

    use indexing::IndexerStatus;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    /// Port 1, where nothing listens. Liveness never asks either database.
    fn unreachable() -> probes::Dependencies {
        let clickhouse = clickhouse_client::build_client(clickhouse_client::Config {
            url: "http://127.0.0.1:1".to_owned(),
            transport_security: clickhouse_client::TransportSecurity::AllowInsecureHttp,
            database: "default".to_owned(),
            user: "default".to_owned(),
            password: String::new(),
            max_execution: Duration::from_secs(1),
        })
        .unwrap();

        probes::dependencies(
            Stall::new(Arc::new(IndexerStatus::new()), Duration::from_secs(300)),
            clickhouse,
            postgres::probe_pool(
                "postgres://postgres@127.0.0.1:1/postgres?sslmode=disable",
                postgres::Limits::default(),
            )
            .unwrap(),
        )
    }

    #[tokio::test]
    async fn serves_over_a_socket_and_stops_when_drained() {
        let shutdown = ShutdownFlag::new();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (terminate, terminated) = tokio::sync::oneshot::channel::<()>();

        let served = tokio::spawn({
            let router = probes::routes(Uptime::now(), shutdown.clone(), unreachable());
            let shutdown = shutdown.clone();
            async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(async move {
                        let _ = terminated.await;
                        shutdown.begin_and_hold(Duration::ZERO).await;
                    })
                    .await
                    .unwrap();
            }
        });

        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        socket
            .write_all(b"GET /health/live HTTP/1.1\r\nHost: probe\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();

        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(
            response.ends_with(r#"{"status":"ok","uptime_seconds":0}"#),
            "{response}"
        );

        // Resolving the future ends the accept loop; ignoring it would hang.
        terminate.send(()).unwrap();
        served.await.unwrap();
    }
}
