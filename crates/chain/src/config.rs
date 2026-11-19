use std::time::Duration;

use env::{Env, Invalid};

pub struct Config {
    /// A preference order, not a pool: the first is tried first.
    pub urls: Vec<String>,
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
        Ok(Self {
            urls: env.urls("RPC_URLS")?,
            timeout: env.millis("RPC_TIMEOUT_MS", 10_000, 100, 120_000)?,
        })
    }
}
