use std::time::Duration;

use env::{Env, Invalid};

/// How enrichment paces itself, read from the variables it owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnrichmentSettings {
    /// How long to wait after a run that left a gap open. A run that closed every gap imposes none.
    pub retry_delay: Duration,

    /// How many tokens are read at once. A public endpoint rate-limits a burst long before
    /// seventeen reads become slow.
    pub concurrency: usize,
}

impl EnrichmentSettings {
    /// # Errors
    ///
    /// [`Invalid`], naming the first variable out of its bounds.
    pub fn read(env: &Env) -> Result<Self, Invalid> {
        Ok(Self {
            retry_delay: env.millis("TOKEN_ENRICHMENT_RETRY_MS", 60_000, 1_000, 86_400_000)?,
            concurrency: env
                .count("TOKEN_ENRICHMENT_CONCURRENCY", 4, 1, 64)?
                .try_into()
                .unwrap_or(64),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(pairs: &[(&str, &str)]) -> Result<EnrichmentSettings, Invalid> {
        EnrichmentSettings::read(&pairs.iter().copied().collect())
    }

    #[test]
    fn an_empty_environment_is_a_minute_between_retries_and_four_reads_at_once() {
        assert_eq!(
            read(&[]).unwrap(),
            EnrichmentSettings {
                retry_delay: Duration::from_secs(60),
                concurrency: 4,
            }
        );
    }

    #[test]
    fn takes_both_variables_at_their_bounds() {
        let lowest = read(&[
            ("TOKEN_ENRICHMENT_RETRY_MS", "1000"),
            ("TOKEN_ENRICHMENT_CONCURRENCY", "1"),
        ])
        .unwrap();
        let highest = read(&[
            ("TOKEN_ENRICHMENT_RETRY_MS", "86400000"),
            ("TOKEN_ENRICHMENT_CONCURRENCY", "64"),
        ])
        .unwrap();

        assert_eq!(
            (lowest.retry_delay, lowest.concurrency),
            (Duration::from_secs(1), 1)
        );
        assert_eq!(
            (highest.retry_delay, highest.concurrency),
            (Duration::from_secs(86_400), 64)
        );
    }

    #[test]
    fn refuses_each_variable_past_its_bounds() {
        for (key, value) in [
            ("TOKEN_ENRICHMENT_RETRY_MS", "999"),
            ("TOKEN_ENRICHMENT_RETRY_MS", "86400001"),
            ("TOKEN_ENRICHMENT_CONCURRENCY", "0"),
            ("TOKEN_ENRICHMENT_CONCURRENCY", "65"),
        ] {
            let refusal = read(&[(key, value)]).unwrap_err().to_string();

            assert!(refusal.contains(key), "{key}={value}: {refusal}");
        }
    }
}
