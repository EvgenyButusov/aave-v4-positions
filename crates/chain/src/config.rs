use std::time::Duration;

use env::{Env, Invalid, Urls};
use url::Url;

pub struct Config {
    pub preferred: Url,
    /// Tried in this order when the preferred provider fails.
    pub fallbacks: Vec<Url>,
    /// Per provider and per call, so a list of three can take three times this.
    pub timeout: Duration,
}

impl Config {
    /// The `RPC_*` group.
    ///
    /// # Errors
    ///
    /// [`Invalid`], naming the first variable that could not be read.
    pub fn read(env: &Env) -> Result<Self, Invalid> {
        let Urls { first, rest } = env.urls("RPC_URLS")?;

        Ok(Self {
            preferred: first,
            fallbacks: rest,
            timeout: env.millis("RPC_TIMEOUT_MS", 10_000, 100, 120_000)?,
        })
    }
}
