//! What this process is set up to be, parsed once, before anything else.
//!
//! The API's own variables; each shared group is read by the crate it configures.
//!
//! `POSITIONS_CURSOR_SECRET` has no default and refuses rather than falls back.

use std::net::IpAddr;
use std::time::Duration;

use env::{Env, Invalid, Source};

use crate::positions::MIN_SECRET_BYTES;

/// A bound, not a units check: it catches `300000` and not `60000`, which is
/// just as wrong. The name carries the unit; this only stops the absurd.
const MAX_STALENESS_SECONDS: u64 = 86_400;

pub(crate) struct Config {
    pub(crate) telemetry: telemetry::Settings,
    pub(crate) host: IpAddr,
    pub(crate) port: u16,
    pub(crate) grace: Duration,
    /// What the versioned routes are mounted under. The probes are not: an
    /// orchestrator's check is not part of the API's versioned surface, and
    /// every compose healthcheck already asks for `/health/ready`.
    pub(crate) prefix: String,

    /// Outside the prefix, and always served.
    pub(crate) docs_path: String,

    /// On disk, copied in by the image. Absent is the document with no viewer
    /// in front of it, which is what a `cargo run` gets.
    pub(crate) docs_assets: String,
    pub(crate) cursor_secret: String,
    pub(crate) staleness: Staleness,
    pub(crate) clickhouse: clickhouse_client::Config,
    pub(crate) postgres_url: String,
    pub(crate) postgres_limits: postgres::Limits,

    pub(crate) request_timeout: Duration,
}

/// How old a number may be before a page says so.
///
/// Two rather than one, because the indexer advances every block and the oracle
/// is read every minute. Neither is the indexer's `INDEXER_STALL_THRESHOLD_MS`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Staleness {
    pub(crate) sync: u64,

    /// How long since we last read the oracle, never since a feed moved
    /// (§7.5): an hour without an `AnswerUpdated` is ordinary Chainlink.
    pub(crate) price: u64,
}

impl Config {
    /// Which files [`Source`] reads is decided by `APP_ENV`, not by how the
    /// binary was built.
    ///
    /// # Errors
    ///
    /// [`Invalid`], naming the first variable that could not be read.
    pub(crate) fn from_env() -> Result<Self, Invalid> {
        Self::parse(&Source::from_env().read())
    }

    fn parse(env: &Env) -> Result<Self, Invalid> {
        Ok(Self {
            telemetry: telemetry::Settings::read(env)?,
            host: env.address("API_HOST", "0.0.0.0")?,
            port: env.port("API_PORT", 3000)?,
            grace: Duration::from_secs(env.seconds("SHUTDOWN_GRACE_SECONDS", 10, 300)?),
            prefix: env.text("API_GLOBAL_PREFIX", "api"),
            docs_path: env.text("API_DOCS_PATH", "docs"),
            docs_assets: env.text("API_DOCS_ASSETS", "/usr/share/api/docs"),
            cursor_secret: env.secret("POSITIONS_CURSOR_SECRET", MIN_SECRET_BYTES)?,
            staleness: Staleness {
                sync: env.seconds("API_SYNC_STALE_AFTER_SECONDS", 60, MAX_STALENESS_SECONDS)?,
                price: env.seconds("API_PRICE_STALE_AFTER_SECONDS", 300, MAX_STALENESS_SECONDS)?,
            },
            clickhouse: clickhouse_client::Config::read(env)?,
            postgres_url: postgres::read_url(env)?,
            postgres_limits: postgres::Limits::read(env)?,
            request_timeout: env.deadline("API_REQUEST_TIMEOUT_MS", Duration::from_secs(10))?,
        })
    }
}

#[cfg(test)]
mod tests {
    //! The mapping, not the readers: which variable reaches which field, and
    //! what a deployment that sets none of them gets.

    use tracing::level_filters::LevelFilter;

    use super::*;

    /// Thirty-two bytes, which is what the cursor codec asks for.
    const SECRET: (&str, &str) = (
        "POSITIONS_CURSOR_SECRET",
        "a-test-key-that-is-long-enough!!",
    );

    fn parse(pairs: &[(&str, &str)]) -> Result<Config, Invalid> {
        Config::parse(&pairs.iter().copied().collect())
    }

    /// The two variables with no default, which every other case needs set.
    fn configured(pairs: &[(&str, &str)]) -> Result<Config, Invalid> {
        let mut all = pairs.to_vec();
        all.push(SECRET);
        all.push(("OTEL_SERVICE_NAME", "api-rust"));

        parse(&all)
    }

    #[test]
    fn an_empty_environment_is_the_local_defaults() {
        let config = configured(&[]).expect("defaults should stand alone");

        assert_eq!(config.host, IpAddr::from([0, 0, 0, 0]));
        assert_eq!(config.port, 3000);
        assert_eq!(config.grace, Duration::from_secs(10));
        assert_eq!(config.prefix, "api");
        assert_eq!(config.docs_path, "docs");
        assert_eq!(config.docs_assets, "/usr/share/api/docs");
        assert_eq!(config.staleness.sync, 60);
        assert_eq!(config.staleness.price, 300);
        assert_eq!(config.request_timeout, Duration::from_secs(10));
    }

    #[test]
    fn refuses_to_boot_without_a_cursor_secret() {
        // A shared default key is not a signature: the alternative to this
        // refusal is a process serving forgeable cursors.
        // The service name is set because telemetry is read first.
        let refusal = parse(&[("OTEL_SERVICE_NAME", "api-rust")])
            .err()
            .expect("expected a refusal")
            .to_string();

        assert!(refusal.contains("POSITIONS_CURSOR_SECRET"), "{refusal}");
        assert!(!refusal.contains("a-test-key"), "the key reached the log");
    }

    #[test]
    fn the_first_bad_variable_in_reading_order_is_the_one_reported() {
        // Fields evaluate in source order, so this pins the order a deployment
        // reads its refusals in. Both are wrong; the one named is read first.
        let refusal = configured(&[("API_PORT", "0"), ("SHUTDOWN_GRACE_SECONDS", "600")])
            .err()
            .expect("expected a refusal")
            .to_string();

        assert!(
            refusal.ends_with("API_PORT: must be 1..=65535, got \"0\""),
            "{refusal}"
        );
    }

    #[test]
    fn bounds_a_staleness_threshold_without_pretending_to_check_its_units() {
        // Both halves: the price default in milliseconds is refused, the sync
        // default in milliseconds is taken and just as wrong.
        let refusal = configured(&[("API_PRICE_STALE_AFTER_SECONDS", "300000")])
            .err()
            .expect("expected a refusal")
            .to_string();

        assert!(refusal.contains("must be 0..=86400"), "{refusal}");

        let taken = configured(&[("API_SYNC_STALE_AFTER_SECONDS", "60000")])
            .expect("under a day, and still wrong");

        assert_eq!(taken.staleness.sync, 60_000);
    }

    #[test]
    fn reads_every_variable_it_declares_and_one_from_each_group() {
        let config = configured(&[
            ("API_HOST", "127.0.0.1"),
            ("API_PORT", "8080"),
            ("SHUTDOWN_GRACE_SECONDS", "5"),
            ("API_GLOBAL_PREFIX", "gateway"),
            ("API_DOCS_PATH", "reference"),
            ("API_DOCS_ASSETS", "/srv/docs"),
            ("API_SYNC_STALE_AFTER_SECONDS", "30"),
            ("API_PRICE_STALE_AFTER_SECONDS", "900"),
            ("API_REQUEST_TIMEOUT_MS", "2500"),
            ("LOG_LEVEL", "debug"),
            ("CLICKHOUSE_DATABASE", "aave"),
            ("POSTGRES_URL", "postgres://aave@postgres:5432/aave"),
            ("POSTGRES_MAX_CONNECTIONS", "8"),
        ])
        .expect("every value is valid");

        assert_eq!(config.host, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(config.port, 8080);
        assert_eq!(config.grace, Duration::from_secs(5));
        assert_eq!(config.prefix, "gateway");
        assert_eq!(config.docs_path, "reference");
        assert_eq!(config.docs_assets, "/srv/docs");
        assert_eq!(config.cursor_secret, SECRET.1);
        assert_eq!(config.staleness.sync, 30);
        assert_eq!(config.staleness.price, 900);
        assert_eq!(config.request_timeout, Duration::from_millis(2_500));
        assert_eq!(config.telemetry.service, "api-rust");
        assert_eq!(config.telemetry.level, LevelFilter::DEBUG);
        assert_eq!(config.clickhouse.database, "aave");
        assert_eq!(config.postgres_url, "postgres://aave@postgres:5432/aave");
        assert_eq!(config.postgres_limits.max_size, 8);
    }
}
