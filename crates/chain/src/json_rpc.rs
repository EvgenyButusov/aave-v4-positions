#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap};

use alloy_primitives::{Address, B256, Bytes, U64};
use alloy_rpc_client::RpcClient;
use alloy_transport::TransportError;
use async_trait::async_trait;
use futures_util::future::try_join_all;
use indexing::{BlockHeader, ChainClient, ChainError, LogFilter, LogReader, RawLog};
use opentelemetry::metrics::Meter;
use serde::{Deserialize, Serialize};
use units::ChainId;

use crate::config::Config;
use crate::error::Error;
use crate::http;
use crate::providers::Providers;
use crate::refusal::{detail, is_range_rejection};

/// Both chain ports over one provider list, so the loop and a processor fail over together.
#[derive(Clone)]
pub struct JsonRpc {
    client: RpcClient,
}

/// What a node answered that no node should.
#[derive(Debug, thiserror::Error)]
enum Unexpected {
    #[error("the provider reports chain id {0}, which no chain can have")]
    ChainId(u64),

    #[error("the provider has no block {0}, below a head it reported")]
    NoBlock(u64),

    #[error("eth_getLogs returned a log with no block for {from}..={to}")]
    Pending { from: u64, to: u64 },
}

/// Four fields, so a chain whose header adds or omits the rest still parses.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    number: U64,
    hash: B256,
    parent_hash: B256,
    timestamp: U64,
}

/// `topics` is omitted rather than empty when nothing narrows the signature.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogQuery {
    address: Vec<Address>,
    from_block: U64,
    to_block: U64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    topics: Vec<Vec<B256>>,
}

/// Every block field is optional on the wire, because the same shape carries a pending log.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Log {
    address: Address,
    topics: Vec<B256>,
    data: Bytes,
    block_number: Option<U64>,
    block_hash: Option<B256>,
    block_timestamp: Option<U64>,
    transaction_hash: Option<B256>,
    transaction_index: Option<U64>,
    log_index: Option<U64>,
}

impl JsonRpc {
    /// Does no I/O, so a pod whose every provider is down boots and reports not-ready rather than
    /// crash-looping.
    ///
    /// # Errors
    ///
    /// [`Error`] when the list is empty or unparsable, or no HTTP client can be built.
    pub fn connect(config: &Config, meter: &Meter) -> Result<Self, Error> {
        let providers = Providers::new(&config.urls, &http::client(config.timeout)?, meter)?;

        Ok(Self {
            client: RpcClient::new(providers, false),
        })
    }

    async fn timestamps(&self, logs: &[Log]) -> Result<HashMap<u64, u64>, ChainError> {
        let missing: BTreeSet<u64> = logs
            .iter()
            .filter(|log| log.block_timestamp.is_none())
            .filter_map(|log| log.block_number.map(|number| number.to()))
            .collect();

        if missing.is_empty() {
            return Ok(HashMap::new());
        }

        tracing::warn!(
            headers = missing.len(),
            "provider omitted blockTimestamp; reading block headers to fill it"
        );
        let headers =
            try_join_all(missing.into_iter().map(|number| self.block_header(number))).await?;

        Ok(headers
            .into_iter()
            .map(|header| (header.number, header.timestamp))
            .collect())
    }
}

#[async_trait]
impl ChainClient for JsonRpc {
    async fn chain_id(&self) -> Result<ChainId, ChainError> {
        let reported: U64 = self
            .client
            .request_noparams("eth_chainId")
            .await
            .map_err(unavailable)?;
        let reported = reported.to();

        ChainId::new(reported).map_err(|_| unavailable(Unexpected::ChainId(reported)))
    }

    async fn head_block_number(&self) -> Result<u64, ChainError> {
        let head: U64 = self
            .client
            .request_noparams("eth_blockNumber")
            .await
            .map_err(unavailable)?;

        Ok(head.to())
    }

    async fn block_header(&self, number: u64) -> Result<BlockHeader, ChainError> {
        let header: Option<Header> = self
            .client
            .request("eth_getBlockByNumber", (U64::from(number), false))
            .await
            .map_err(unavailable)?;
        let header = header.ok_or_else(|| unavailable(Unexpected::NoBlock(number)))?;

        Ok(BlockHeader {
            number: header.number.to(),
            hash: header.hash,
            parent_hash: header.parent_hash,
            timestamp: header.timestamp.to(),
        })
    }
}

#[async_trait]
impl LogReader for JsonRpc {
    async fn logs(&self, filter: &LogFilter) -> Result<Vec<RawLog>, ChainError> {
        let query = LogQuery {
            address: filter.addresses.clone(),
            from_block: U64::from(filter.from_block),
            to_block: U64::from(filter.to_block),
            topics: if filter.topic0.is_empty() {
                Vec::new()
            } else {
                vec![filter.topic0.clone()]
            },
        };

        let logs: Vec<Log> = self
            .client
            .request("eth_getLogs", (query,))
            .await
            .map_err(|error| refused(filter, error))?;
        let filled = self.timestamps(&logs).await?;

        logs.into_iter()
            .map(|log| {
                mined(log, &filled).ok_or_else(|| {
                    unavailable(Unexpected::Pending {
                        from: filter.from_block,
                        to: filter.to_block,
                    })
                })
            })
            .collect()
    }
}

/// `None` for a log that belongs to no block, which a bounded range cannot return.
fn mined(log: Log, filled: &HashMap<u64, u64>) -> Option<RawLog> {
    let block_number = log.block_number?.to();

    Some(RawLog {
        address: log.address,
        topics: log.topics,
        data: log.data,
        block_number,
        block_hash: log.block_hash?,
        block_timestamp: log
            .block_timestamp
            .map(|timestamp| timestamp.to())
            .or_else(|| filled.get(&block_number).copied())?,
        transaction_hash: log.transaction_hash?,
        transaction_index: log.transaction_index?.to(),
        log_index: log.log_index?.to(),
    })
}

fn refused(filter: &LogFilter, error: TransportError) -> ChainError {
    match detail(&error) {
        Some(detail) if is_range_rejection(detail) => ChainError::RangeTooLarge {
            from: filter.from_block,
            to: filter.to_block,
            detail: detail.to_owned(),
        },
        _ => unavailable(error),
    }
}

fn unavailable(source: impl std::error::Error + Send + Sync + 'static) -> ChainError {
    ChainError::Unavailable {
        source: Box::new(source),
    }
}
