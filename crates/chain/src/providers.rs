//! The provider list as one transport: each tried once, in the order given, until one answers.
//!
//! alloy's `FallbackLayer` was read and declined: it ranks providers by measured latency and
//! races the top few in parallel, where this list is a stated preference and a race spends every
//! provider's quota on each call.

mod health;
#[cfg(test)]
mod tests;

use std::iter;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use alloy_json_rpc::{RequestPacket, ResponsePacket};
use alloy_transport::{TransportError, TransportFut};
use alloy_transport_http::Http;
use opentelemetry::metrics::Meter;
use tower::{Service, ServiceExt as _};
use tracing::Instrument as _;
use url::Url;

use health::{Answer, Health};

#[derive(Clone)]
pub(crate) struct Providers(Arc<Members>);

struct Members {
    preferred: Http<reqwest::Client>,
    fallbacks: Vec<Http<reqwest::Client>>,
    health: Health,
}

impl Providers {
    pub(crate) fn new(
        preferred: &Url,
        fallbacks: &[Url],
        client: &reqwest::Client,
        meter: &Meter,
    ) -> Self {
        let http = |url: &Url| Http::with_client(client.clone(), url.clone());
        let labels = iter::once(preferred).chain(fallbacks).map(label).collect();

        Self(Arc::new(Members {
            preferred: http(preferred),
            fallbacks: fallbacks.iter().map(http).collect(),
            health: Health::new(meter, labels),
        }))
    }
}

impl Members {
    async fn send(&self, request: RequestPacket) -> Result<ResponsePacket, TransportError> {
        let method = method_of(&request);

        let mut answer = self
            .attempt(0, &self.preferred, &method, request.clone())
            .await;
        for (index, member) in (1..).zip(&self.fallbacks) {
            if Answer::of(&answer).settles() {
                break;
            }
            answer = self.attempt(index, member, &method, request.clone()).await;
        }

        answer
    }

    async fn attempt(
        &self,
        index: usize,
        member: &Http<reqwest::Client>,
        method: &str,
        request: RequestPacket,
    ) -> Result<ResponsePacket, TransportError> {
        let span = tracing::info_span!(
            "rpc",
            otel.kind = "client",
            rpc.method = method,
            rpc.provider = self.health.label(index),
        );

        let started = Instant::now();
        let answer = member.clone().oneshot(request).instrument(span).await;
        self.health
            .record(index, method, started.elapsed(), &Answer::of(&answer));

        answer
    }
}

impl Service<RequestPacket> for Providers {
    type Response = ResponsePacket;
    type Error = TransportError;
    type Future = TransportFut<'static>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: RequestPacket) -> Self::Future {
        let members = Arc::clone(&self.0);

        Box::pin(async move { members.send(request).await })
    }
}

/// The host and a non-default port, and never the path: a provider's key is often in it.
fn label(url: &Url) -> String {
    let host = url.host_str().unwrap_or("unknown");

    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

/// A batch is named a batch: naming it after its first call would put every method it carries
/// under one.
fn method_of(request: &RequestPacket) -> String {
    match request {
        RequestPacket::Single(single) => single.method().to_owned(),
        RequestPacket::Batch(_) => "batch".to_owned(),
    }
}
