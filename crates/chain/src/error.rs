/// Why the providers could not be set up. Nothing here reaches the network, so an unreachable
/// provider is never one of these.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("RPC_URLS lists no provider")]
    NoProvider,

    /// Carries the entry's position and never its text, which holds the provider's key.
    #[error("RPC_URLS entry {entry} is not a URL")]
    NotAUrl { entry: usize },

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
