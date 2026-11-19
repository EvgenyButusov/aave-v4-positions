//! Who is answering, and who has stopped. Without it a dead preferred provider is silent: the next
//! one serves every call and nothing says so.

use std::sync::Mutex;
use std::time::Duration;

use alloy_json_rpc::{ErrorPayload, ResponsePacket};
use alloy_transport::{RpcError, TransportError, TransportErrorKind};
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter};

pub(super) struct Health {
    labels: Vec<String>,
    requests: Counter<u64>,
    errors: Counter<u64>,
    failovers: Counter<u64>,
    duration: Histogram<f64>,
    /// Who last answered, so a failover is logged on the transition rather than on every call a
    /// dead provider is retried on, which would bury the line that matters.
    serving: Mutex<Option<usize>>,
}

/// One provider's answer to one call, as the list and the instruments read it.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Answer {
    Served,
    Refused { status: u16, error_type: String },
    Failed { error_type: String },
}

impl Answer {
    pub(super) fn of(result: &Result<ResponsePacket, TransportError>) -> Self {
        match result {
            Ok(packet) => match packet.as_error() {
                Some(error) if !is_revert(error) => Self::Refused {
                    status: 200,
                    error_type: error.code.to_string(),
                },
                _ => Self::Served,
            },
            Err(RpcError::Transport(
                TransportErrorKind::HttpError(http)
                | TransportErrorKind::HttpErrorWithRetryAfter { error: http, .. },
            )) => Self::Refused {
                status: http.status,
                error_type: http.status.to_string(),
            },
            Err(error) => Self::Failed {
                error_type: failure(error).to_owned(),
            },
        }
    }

    pub(super) fn settles(&self) -> bool {
        *self == Self::Served
    }
}

/// The contract's answer rather than the node's, so every provider would give the same one.
fn is_revert(error: &ErrorPayload) -> bool {
    error.message.contains("execution reverted")
        || error.message.contains("gas required exceeds allowance")
}

fn failure(error: &TransportError) -> &'static str {
    match error {
        RpcError::Transport(TransportErrorKind::Custom(source)) => {
            match source.downcast_ref::<reqwest::Error>() {
                Some(cause) if cause.is_timeout() => "timeout",
                Some(cause) if cause.is_connect() => "connect",
                _ => "transport",
            }
        }
        RpcError::DeserError { .. } => "deserialize",
        _ => "transport",
    }
}

impl Health {
    pub(super) fn new(meter: &Meter, labels: Vec<String>) -> Self {
        Self {
            labels,
            requests: meter
                .u64_counter("rpc.client.requests")
                .with_description("RPC calls by provider, method and status. Which provider is answering.")
                .build(),
            errors: meter
                .u64_counter("rpc.client.errors")
                .with_description("Timeouts, 429s, 5xx and JSON-RPC refusals kept apart, by provider.")
                .build(),
            failovers: meter
                .u64_counter("rpc.client.failovers")
                .with_description("A handover to a provider other than the preferred one: the signal a silent failover lacked.")
                .build(),
            duration: meter
                .f64_histogram("rpc.client.duration")
                .with_unit("s")
                .with_description("Per provider and per JSON-RPC method, which the URL alone cannot tell you.")
                .build(),
            serving: Mutex::new(None),
        }
    }

    pub(super) fn label(&self, index: usize) -> &str {
        self.labels.get(index).map_or("unknown", String::as_str)
    }

    pub(super) fn record(&self, index: usize, method: &str, elapsed: Duration, answer: &Answer) {
        let attributes = [
            KeyValue::new("provider", self.label(index).to_owned()),
            KeyValue::new("preferred", index == 0),
            KeyValue::new("rpc.method", method.to_owned()),
        ];
        self.duration.record(elapsed.as_secs_f64(), &attributes);

        let with = |key: &'static str, value: opentelemetry::Value| {
            let mut extended = attributes.to_vec();
            extended.push(KeyValue::new(key, value));
            extended
        };

        match answer {
            Answer::Served => {
                self.requests.add(1, &with("http.status_code", 200.into()));
                self.observe_serving(index);
            }
            Answer::Refused { status, error_type } => {
                self.requests
                    .add(1, &with("http.status_code", i64::from(*status).into()));
                self.errors
                    .add(1, &with("error.type", error_type.clone().into()));
            }
            Answer::Failed { error_type } => {
                self.errors
                    .add(1, &with("error.type", error_type.clone().into()));
            }
        }
    }

    /// Only on an answer: a provider that errored has not taken over, the next one is about to be
    /// asked. The first answer is the baseline and not a failover, or every start would count one.
    fn observe_serving(&self, index: usize) {
        let previous = self
            .serving
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .replace(index);

        let Some(previous) = previous.filter(|&previous| previous != index) else {
            return;
        };

        let provider = self.label(index);
        let previous_provider = self.label(previous);

        if index == 0 {
            tracing::info!(provider, previous_provider, "rpc provider recovered");
            return;
        }

        self.failovers.add(
            1,
            &[
                KeyValue::new("provider", provider.to_owned()),
                KeyValue::new("previous_provider", previous_provider.to_owned()),
            ],
        );
        tracing::warn!(provider, previous_provider, "rpc provider failover");
    }
}
