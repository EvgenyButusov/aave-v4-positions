//! What one request produced in spans and log lines.
//!
//! Each case gets a subscriber of its own, so they run in parallel without
//! reading each other's telemetry. Nothing here knows what this service does —
//! it takes a router and hands back what came out of it.

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SpanData};
use test_tracing::Captured;
use tower::ServiceExt;
use tracing_subscriber::layer::SubscriberExt as _;

pub(crate) struct Observed {
    pub(crate) request_id: Option<String>,
    pub(crate) spans: Vec<SpanData>,

    pub(crate) logged: String,
}

pub(crate) fn observed(router: Router, request: Request<Body>) -> Observed {
    test_tracing::interesting();

    // A global by design, and every case sets the same one.
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let (captured, lines) = Captured::new();
    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(captured),
        )
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer(telemetry::SCOPE)));

    let request_id = tracing::subscriber::with_default(subscriber, || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                router
                    .oneshot(request)
                    .await
                    .unwrap()
                    .headers()
                    .get("x-request-id")
                    .map(|value| value.to_str().unwrap().to_owned())
            })
    });
    provider.force_flush().unwrap();

    Observed {
        request_id,
        spans: exporter.get_finished_spans().unwrap(),
        logged: lines.read(),
    }
}
