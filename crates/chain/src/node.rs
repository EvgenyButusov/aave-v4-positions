//! A provider on a local socket that answers from a script, and records every call it was asked.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use opentelemetry::metrics::MeterProvider as _;
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, PeriodicReader, SdkMeterProvider};
use serde_json::{Value, json};

use crate::config::Config;
use crate::json_rpc::JsonRpc;

pub(crate) enum Reply {
    Result(Value),
    Error { code: i64, message: &'static str },
    Status(u16),
    Hang,
}

type Script = dyn Fn(&str, &Value) -> Reply + Send + Sync;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Call {
    pub(crate) node: String,
    pub(crate) method: String,
    pub(crate) params: Value,
}

#[derive(Clone)]
struct Answering {
    name: String,
    script: Arc<Script>,
    calls: mpsc::Sender<Call>,
}

pub(crate) struct Node {
    pub(crate) url: String,
    pub(crate) label: String,
}

/// Every node a case stands up reports into one of these, so the order across providers is kept.
pub(crate) struct Network {
    calls: mpsc::Sender<Call>,
    received: mpsc::Receiver<Call>,
}

impl Network {
    pub(crate) fn new() -> Self {
        let (calls, received) = mpsc::channel();

        Self { calls, received }
    }

    pub(crate) async fn node(
        &self,
        name: &str,
        script: impl Fn(&str, &Value) -> Reply + Send + Sync + 'static,
    ) -> Node {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = axum::Router::new()
            .route("/", axum::routing::post(answer))
            .with_state(Answering {
                name: name.to_owned(),
                script: Arc::new(script),
                calls: self.calls.clone(),
            });
        tokio::spawn(async move { axum::serve(listener, router).await });

        Node {
            url: format!("http://{address}"),
            label: address.to_string(),
        }
    }

    pub(crate) fn calls(&self) -> Vec<Call> {
        self.received.try_iter().collect()
    }

    pub(crate) fn asked(&self) -> Vec<(String, String)> {
        self.calls()
            .into_iter()
            .map(|call| (call.node, call.method))
            .collect()
    }
}

async fn answer(State(answering): State<Answering>, Json(request): Json<Value>) -> Response {
    let method = request["method"].as_str().unwrap_or_default().to_owned();
    let params = request["params"].clone();
    let _ = answering.calls.send(Call {
        node: answering.name.clone(),
        method: method.clone(),
        params: params.clone(),
    });

    let id = request["id"].clone();
    match (answering.script)(&method, &params) {
        Reply::Result(result) => {
            Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
        }
        Reply::Error { code, message } => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message },
        }))
        .into_response(),
        Reply::Status(status) => {
            (StatusCode::from_u16(status).unwrap(), "upstream error").into_response()
        }
        Reply::Hang => std::future::pending().await,
    }
}

/// Answers by how many calls this node was asked before this one.
pub(crate) fn in_turn(
    reply: impl Fn(usize) -> Reply + Send + Sync + 'static,
) -> impl Fn(&str, &Value) -> Reply + Send + Sync + 'static {
    let asked = AtomicUsize::new(0);

    move |_, _| reply(asked.fetch_add(1, Ordering::Relaxed))
}

/// Port 1, where nothing listens: a provider that is down rather than slow.
pub(crate) const UNREACHABLE: &str = "http://127.0.0.1:1";

pub(crate) struct Client {
    pub(crate) rpc: JsonRpc,
    pub(crate) exporter: InMemoryMetricExporter,
    pub(crate) meters: SdkMeterProvider,
}

pub(crate) fn client(urls: &[&str]) -> Client {
    client_within(urls, Duration::from_secs(5))
}

pub(crate) fn client_within(urls: &[&str], timeout: Duration) -> Client {
    let exporter = InMemoryMetricExporter::default();
    let meters = SdkMeterProvider::builder()
        .with_reader(PeriodicReader::builder(exporter.clone()).build())
        .build();
    let config = Config {
        urls: urls.iter().map(|url| (*url).to_owned()).collect(),
        timeout,
    };

    Client {
        rpc: JsonRpc::connect(&config, &meters.meter("test")).unwrap(),
        exporter,
        meters,
    }
}

pub(crate) fn hex(number: u64) -> Value {
    Value::String(format!("{number:#x}"))
}
