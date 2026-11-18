//! What this process is set up to follow, parsed once, before anything else.
//!
//! The indexer's own variables; each shared group is read by the crate it configures.

use std::net::IpAddr;
use std::time::Duration;

use alloy_primitives::{Address, address};
use env::{Env, Invalid, Source};
use units::ChainId;

/// The Core Hub's first log, eight blocks before the Main Spoke's: the floor has to be the
/// earliest of the contracts followed, or the first `AddAsset` is missed and reads as a quiet chain.
const CORE_HUB_GENESIS_BLOCK: u64 = 24_720_891;

const MAIN_SPOKE: Address = address!("0x94e7a5dcbe816e498b89ab752661904e2f56c485");
const CORE_HUB: Address = address!("0xcca852bc40e560adc3b1cc58ca5b55638ce826c9");

/// A day: a bound against the absurd, and not a units check.
const MAX_STALL_THRESHOLD_MS: u64 = 86_400_000;

pub(crate) struct Config {
    pub(crate) telemetry: telemetry::Settings,
    pub(crate) host: IpAddr,
    pub(crate) port: u16,
    pub(crate) grace: Duration,
    pub(crate) chain: indexing::Options,
    pub(crate) finality_depth: u64,
    pub(crate) rpc: Rpc,
    pub(crate) stall_threshold: Duration,
    pub(crate) autostart: bool,
    pub(crate) spoke: Address,
    pub(crate) hub: Address,
    pub(crate) clickhouse: clickhouse_client::Config,
    pub(crate) postgres_url: String,
    pub(crate) postgres_limits: postgres::Limits,
}

pub(crate) struct Rpc {
    /// A preference order, not a pool: the first is tried first.
    pub(crate) urls: Vec<String>,
    pub(crate) timeout: Duration,
}

impl Config {
    /// # Errors
    ///
    /// [`Invalid`], naming the first variable that could not be read.
    pub(crate) fn from_env() -> Result<Self, Invalid> {
        Self::parse(&Source::from_env().read())
    }

    fn parse(env: &Env) -> Result<Self, Invalid> {
        Ok(Self {
            telemetry: telemetry::Settings::read(env)?,
            host: env.address("INDEXER_HOST", "0.0.0.0")?,
            port: env.port("INDEXER_PORT", 3001)?,
            grace: Duration::from_secs(env.seconds("SHUTDOWN_GRACE_SECONDS", 10, 300)?),
            chain: indexing::Options {
                chain_id: env.required_as::<ChainId>("CHAIN_ID")?,
                start_block: env.count(
                    "INDEXER_START_BLOCK",
                    CORE_HUB_GENESIS_BLOCK,
                    0,
                    u64::MAX,
                )?,
                max_range_size: env.count("INDEXER_MAX_RANGE_SIZE", 1_000, 1, 100_000)?,
                poll_interval: env.millis("INDEXER_POLL_INTERVAL_MS", 4_000, 100, 600_000)?,
            },
            finality_depth: env.count("FINALITY_DEPTH", 128, 0, 10_000)?,
            rpc: Rpc {
                urls: env.urls("RPC_URLS")?,
                timeout: env.millis("INDEXER_RPC_TIMEOUT_MS", 10_000, 100, 120_000)?,
            },
            stall_threshold: env.millis(
                "INDEXER_STALL_THRESHOLD_MS",
                300_000,
                1_000,
                MAX_STALL_THRESHOLD_MS,
            )?,
            autostart: env.flag("INDEXER_AUTOSTART", true)?,
            spoke: env.address_20("MAIN_SPOKE_ADDRESS", MAIN_SPOKE)?,
            hub: env.address_20("CORE_HUB_ADDRESS", CORE_HUB)?,
            clickhouse: clickhouse_client::Config::read(env)?,
            postgres_url: postgres::read_url(env)?,
            postgres_limits: postgres::Limits::read(env)?,
        })
    }
}

#[cfg(test)]
mod tests {
    //! The mapping, not the readers: which variable reaches which field, and what a deployment
    //! that sets none of them gets.

    use tracing::level_filters::LevelFilter;

    use super::*;

    const REQUIRED: [(&str, &str); 3] = [
        ("OTEL_SERVICE_NAME", "indexer-rust"),
        ("CHAIN_ID", "1"),
        ("RPC_URLS", "https://eth.example/v2/the-providers-key"),
    ];

    fn parse(pairs: &[(&str, &str)]) -> Result<Config, Invalid> {
        Config::parse(&pairs.iter().copied().collect())
    }

    /// Later pairs win, so a case overrides a required one by naming it again.
    fn configured(pairs: &[(&str, &str)]) -> Result<Config, Invalid> {
        parse(&[&REQUIRED[..], pairs].concat())
    }

    fn refusal(pairs: &[(&str, &str)]) -> String {
        configured(pairs)
            .err()
            .expect("expected a refusal")
            .to_string()
    }

    #[test]
    fn an_empty_environment_is_the_local_defaults() {
        let config = configured(&[]).expect("defaults should stand alone");

        assert_eq!(config.host, IpAddr::from([0, 0, 0, 0]));
        assert_eq!(config.port, 3001);
        assert_eq!(config.grace, Duration::from_secs(10));
        assert_eq!(config.chain.start_block, 24_720_891);
        assert_eq!(config.chain.max_range_size, 1_000);
        assert_eq!(config.chain.poll_interval, Duration::from_secs(4));
        assert_eq!(config.finality_depth, 128);
        assert_eq!(config.rpc.timeout, Duration::from_secs(10));
        assert_eq!(config.stall_threshold, Duration::from_secs(300));
        assert!(config.autostart);
        assert_eq!(
            format!("{:#x}", config.spoke),
            "0x94e7a5dcbe816e498b89ab752661904e2f56c485"
        );
        assert_eq!(
            format!("{:#x}", config.hub),
            "0xcca852bc40e560adc3b1cc58ca5b55638ce826c9"
        );
    }

    #[test]
    fn refuses_to_boot_without_a_chain_id() {
        let refusal = parse(&[REQUIRED[0], REQUIRED[2]])
            .err()
            .expect("expected a refusal")
            .to_string();

        assert!(refusal.ends_with("CHAIN_ID: must be set"), "{refusal}");
    }

    #[test]
    fn refuses_a_chain_id_past_the_protocol_bound() {
        let refusal = refusal(&[("CHAIN_ID", "9007199254740992")]);

        assert!(
            refusal.ends_with(r#"CHAIN_ID: must be 1..=9007199254740991, got "9007199254740992""#),
            "{refusal}"
        );
    }

    #[test]
    fn refuses_to_boot_without_a_provider() {
        let refusal = parse(&REQUIRED[..2])
            .err()
            .expect("expected a refusal")
            .to_string();

        assert!(
            refusal.ends_with("RPC_URLS: must list at least one URL"),
            "{refusal}"
        );
    }

    #[test]
    fn a_bad_provider_is_refused_without_its_key_reaching_the_log() {
        let refusal = refusal(&[(
            "RPC_URLS",
            "https://eth.example/v2/the-providers-key,eth.example/v2/the-providers-key",
        )]);

        assert!(
            refusal.ends_with("RPC_URLS: entry 2 is not a URL"),
            "{refusal}"
        );
        assert!(!refusal.contains("the-providers-key"), "{refusal}");
    }

    #[test]
    fn the_first_bad_variable_in_reading_order_is_the_one_reported() {
        let refusal = refusal(&[("INDEXER_PORT", "0"), ("FINALITY_DEPTH", "-1")]);

        assert!(
            refusal.ends_with(r#"INDEXER_PORT: must be 1..=65535, got "0""#),
            "{refusal}"
        );
    }

    #[test]
    fn holds_every_bound_a_deployment_is_already_held_to() {
        let bounds = [
            ("INDEXER_MAX_RANGE_SIZE", "0", "must be 1..=100000"),
            ("INDEXER_MAX_RANGE_SIZE", "100001", "must be 1..=100000"),
            (
                "INDEXER_POLL_INTERVAL_MS",
                "99",
                "must be 100..=600000 milliseconds",
            ),
            (
                "INDEXER_POLL_INTERVAL_MS",
                "600001",
                "must be 100..=600000 milliseconds",
            ),
            ("FINALITY_DEPTH", "10001", "must be 0..=10000"),
            (
                "INDEXER_RPC_TIMEOUT_MS",
                "99",
                "must be 100..=120000 milliseconds",
            ),
            (
                "INDEXER_RPC_TIMEOUT_MS",
                "120001",
                "must be 100..=120000 milliseconds",
            ),
            (
                "INDEXER_STALL_THRESHOLD_MS",
                "999",
                "must be 1000..=86400000 milliseconds",
            ),
            ("INDEXER_START_BLOCK", "-1", "must be 0..="),
            ("SHUTDOWN_GRACE_SECONDS", "301", "must be 0..=300"),
            (
                "MAIN_SPOKE_ADDRESS",
                "0x94e7",
                "must be a 20-byte hex address",
            ),
            (
                "CORE_HUB_ADDRESS",
                "cca852bc40e560adc3b1cc58ca5b55638ce826c9",
                "must be a 20-byte hex address",
            ),
        ];

        for (key, value, wanted) in bounds {
            let refusal = refusal(&[(key, value)]);

            assert!(refusal.contains(&format!("{key}: {wanted}")), "{refusal}");
        }
    }

    #[test]
    fn reads_every_variable_it_declares_and_one_from_each_group() {
        let config = configured(&[
            ("INDEXER_HOST", "127.0.0.1"),
            ("INDEXER_PORT", "3101"),
            ("SHUTDOWN_GRACE_SECONDS", "5"),
            ("CHAIN_ID", "11297108109"),
            ("INDEXER_START_BLOCK", "0"),
            ("INDEXER_MAX_RANGE_SIZE", "50"),
            ("INDEXER_POLL_INTERVAL_MS", "100"),
            ("FINALITY_DEPTH", "0"),
            ("RPC_URLS", "https://a.example, https://b.example"),
            ("INDEXER_RPC_TIMEOUT_MS", "120000"),
            ("INDEXER_STALL_THRESHOLD_MS", "1000"),
            ("INDEXER_AUTOSTART", "false"),
            (
                "MAIN_SPOKE_ADDRESS",
                "0x00000000000000000000000000000000000000AA",
            ),
            (
                "CORE_HUB_ADDRESS",
                "0x00000000000000000000000000000000000000bb",
            ),
            ("LOG_LEVEL", "debug"),
            ("CLICKHOUSE_DATABASE", "aave"),
            ("POSTGRES_URL", "postgres://aave@postgres:5432/aave"),
            ("POSTGRES_MAX_CONNECTIONS", "8"),
        ])
        .expect("every value is valid");

        assert_eq!(config.host, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(config.port, 3101);
        assert_eq!(config.grace, Duration::from_secs(5));
        assert_eq!(config.chain.chain_id.get(), 11_297_108_109);
        assert_eq!(config.chain.start_block, 0);
        assert_eq!(config.chain.max_range_size, 50);
        assert_eq!(config.chain.poll_interval, Duration::from_millis(100));
        assert_eq!(config.finality_depth, 0);
        assert_eq!(config.rpc.urls, ["https://a.example", "https://b.example"]);
        assert_eq!(config.rpc.timeout, Duration::from_secs(120));
        assert_eq!(config.stall_threshold, Duration::from_secs(1));
        assert!(!config.autostart);
        assert_eq!(config.spoke, Address::with_last_byte(0xaa));
        assert_eq!(config.hub, Address::with_last_byte(0xbb));
        assert_eq!(config.telemetry.service, "indexer-rust");
        assert_eq!(config.telemetry.level, LevelFilter::DEBUG);
        assert_eq!(config.clickhouse.database, "aave");
        assert_eq!(config.postgres_url, "postgres://aave@postgres:5432/aave");
        assert_eq!(config.postgres_limits.max_size, 8);
    }
}
