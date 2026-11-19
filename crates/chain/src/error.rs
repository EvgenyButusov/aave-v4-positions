/// Why the HTTP client could not be built. Nothing here reaches the network, so an unreachable
/// provider is never one of these.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the TLS backend supports none of the protocol versions rustls considers safe")]
    NoTlsVersions {
        #[source]
        source: rustls::Error,
    },

    #[error("the HTTP client could not be built")]
    ClientCantBeBuilt {
        #[source]
        source: reqwest::Error,
    },
}
