//! What a process has to have decided before anything can be exported.

use env::{Env, Invalid};
use tracing::level_filters::LevelFilter;

/// The six the specification spells, and no name of our own.
const SAMPLERS: [(&str, Sampling); 6] = [
    ("always_on", Sampling::AlwaysOn),
    ("always_off", Sampling::AlwaysOff),
    ("traceidratio", Sampling::TraceIdRatio),
    ("parentbased_always_on", Sampling::ParentBasedAlwaysOn),
    ("parentbased_always_off", Sampling::ParentBasedAlwaysOff),
    (
        "parentbased_traceidratio",
        Sampling::ParentBasedTraceIdRatio,
    ),
];

/// One parsed configuration, handed down rather than read.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Every signal is grouped by this. A binary refuses to boot without one.
    pub service: String,
    /// Where each signal is posted, paths included.
    pub endpoints: Endpoints,
    pub sampler: Sampling,
    /// Read by the two ratio samplers, ignored by the other four.
    pub ratio: f64,
    pub level: LevelFilter,
    pub pretty: bool,
    /// No providers and no exporters: the formatter alone.
    pub disabled: bool,
}

/// Complete addresses, not a base: `with_endpoint` takes them verbatim, so a base alone posts
/// every batch to `/` and is 404ed in silence.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub traces: String,
    pub metrics: String,
    pub logs: String,
}

/// The six the specification spells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Sampling {
    AlwaysOn,
    AlwaysOff,
    TraceIdRatio,
    ParentBasedAlwaysOn,
    ParentBasedAlwaysOff,
    ParentBasedTraceIdRatio,
}

impl Settings {
    /// The specification's `OTEL_*` names, plus `LOG_LEVEL` and `LOG_PRETTY`.
    ///
    /// `OTEL_SERVICE_NAME` has no default: telemetry attributed to nothing is only noticed in an
    /// incident. `OTEL_SDK_DISABLED=true` is a complete answer without it.
    ///
    /// # Errors
    ///
    /// [`Invalid`], naming the first variable that could not be read.
    pub fn read(env: &Env) -> Result<Self, Invalid> {
        let disabled = env.flag("OTEL_SDK_DISABLED", false)?;

        Ok(Self {
            service: if disabled {
                String::new()
            } else {
                env.required("OTEL_SERVICE_NAME")?
            },
            endpoints: Endpoints::under(
                &env.url("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4318")?,
            ),
            sampler: env.choice(
                "OTEL_TRACES_SAMPLER",
                &SAMPLERS,
                Sampling::ParentBasedAlwaysOn,
            )?,
            ratio: env.ratio("OTEL_TRACES_SAMPLER_ARG", 1.0)?,
            level: env.level("LOG_LEVEL")?,
            pretty: env.flag("LOG_PRETTY", false)?,
            disabled,
        })
    }
}

impl Endpoints {
    /// OTLP/HTTP's own paths over the base `OTEL_EXPORTER_OTLP_ENDPOINT` carries, as the SDK
    /// composes them when it reads that variable itself.
    fn under(base: &str) -> Self {
        let base = base.trim_end_matches('/');

        Self {
            traces: format!("{base}/v1/traces"),
            metrics: format!("{base}/v1/metrics"),
            logs: format!("{base}/v1/logs"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(pairs: &[(&str, &str)]) -> Result<Settings, Invalid> {
        Settings::read(&pairs.iter().copied().collect())
    }

    const NAMED: (&str, &str) = ("OTEL_SERVICE_NAME", "indexer-rust");

    #[test]
    fn a_named_service_with_nothing_else_set_is_the_local_collector() {
        let settings = read(&[NAMED]).expect("the name is the only requirement");

        assert_eq!(settings.service, "indexer-rust");
        assert_eq!(settings.endpoints.traces, "http://localhost:4318/v1/traces");
        assert_eq!(settings.sampler, Sampling::ParentBasedAlwaysOn);
        assert!((settings.ratio - 1.0).abs() < f64::EPSILON);
        assert_eq!(settings.level, LevelFilter::INFO);
        assert!(!settings.pretty);
        assert!(!settings.disabled);
    }

    #[test]
    fn reads_every_variable_in_the_group() {
        let settings = read(&[
            NAMED,
            ("OTEL_SDK_DISABLED", "false"),
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://telemetry:4318"),
            ("OTEL_TRACES_SAMPLER", "parentbased_traceidratio"),
            ("OTEL_TRACES_SAMPLER_ARG", "0.25"),
            ("LOG_LEVEL", "debug"),
            ("LOG_PRETTY", "1"),
        ])
        .expect("every value is valid");

        assert_eq!(
            settings.endpoints.metrics,
            "http://telemetry:4318/v1/metrics"
        );
        assert_eq!(settings.sampler, Sampling::ParentBasedTraceIdRatio);
        assert!((settings.ratio - 0.25).abs() < f64::EPSILON);
        assert_eq!(settings.level, LevelFilter::DEBUG);
        assert!(settings.pretty);
    }

    #[test]
    fn each_signal_gets_the_path_the_collector_is_listening_for() {
        let posted = Endpoints::under("http://telemetry:4318");

        assert_eq!(posted.traces, "http://telemetry:4318/v1/traces");
        assert_eq!(posted.metrics, "http://telemetry:4318/v1/metrics");
        assert_eq!(posted.logs, "http://telemetry:4318/v1/logs");
    }

    #[test]
    fn a_trailing_slash_does_not_become_a_second_one() {
        assert_eq!(
            Endpoints::under("http://telemetry:4318/").logs,
            "http://telemetry:4318/v1/logs"
        );
    }

    #[test]
    fn refuses_to_boot_unnamed_while_telemetry_is_on() {
        let refusal = read(&[]).expect_err("expected a refusal").to_string();

        assert!(
            refusal.contains("OTEL_SERVICE_NAME: must be set"),
            "{refusal}"
        );
    }

    #[test]
    fn a_process_that_wants_no_telemetry_needs_no_name_for_it() {
        let settings = read(&[("OTEL_SDK_DISABLED", "true")]).expect("disabled is complete");

        assert!(settings.disabled);
        assert_eq!(settings.service, "");
    }

    #[test]
    fn takes_the_samplers_the_specification_spells_and_no_others() {
        let sampler = |value| read(&[NAMED, ("OTEL_TRACES_SAMPLER", value)]).map(|s| s.sampler);

        assert_eq!(sampler("always_off").expect("spelled"), Sampling::AlwaysOff);
        assert_eq!(
            sampler("traceidratio").expect("spelled"),
            Sampling::TraceIdRatio
        );

        let refusal = sampler("parentbased_jaeger_remote")
            .expect_err("expected a refusal")
            .to_string();

        assert!(
            refusal.contains("OTEL_TRACES_SAMPLER: must be one of"),
            "{refusal}"
        );
    }
}
