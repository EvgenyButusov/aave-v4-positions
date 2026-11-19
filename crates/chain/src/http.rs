use std::sync::Arc;
use std::time::Duration;

use rustls::crypto::ring;
use rustls::{ClientConfig, RootCertStore};

use crate::error::Error;

/// The only way to build one: with no crypto provider of its own, a client built without this
/// TLS configuration panics.
pub(crate) fn client(timeout: Duration) -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .use_preconfigured_tls(tls()?)
        .timeout(timeout)
        .build()
        .map_err(|source| Error::Client { source })
}

/// The host's roots under `ring`, named rather than resolved: a process default derived from
/// the graph's features panics on first use once a second crypto backend is linked.
fn tls() -> Result<ClientConfig, Error> {
    let mut store = RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();

    if native.certs.is_empty() {
        tracing::warn!(
            errors = native.errors.len(),
            "no certificate roots on this host; every https provider will be refused"
        );
    }
    store.add_parsable_certificates(native.certs);

    Ok(
        ClientConfig::builder_with_provider(Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|source| Error::NoTlsVersions { source })?
            .with_root_certificates(store)
            .with_no_client_auth(),
    )
}
