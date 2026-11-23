//! The loop's collaborators: one chain client shared by the loop, the detector and the processors,
//! the cursor and header window in Postgres, and one processor per followed contract.

#[cfg(test)]
mod tests;

use std::sync::Arc;

use aave::events::Contract;
use aave_clickhouse::ClickHouseEventLedger;
use alloy_primitives::Address;
use chain::JsonRpc;
use clickhouse_client::clickhouse::Client;
use indexing::{
    BlockProcessor, HashChainReorgDetector, IndexerStatus, Instruments, LogReader, Pipeline,
    PostgresBlockHeaderStore, PostgresCursorStore,
};
use opentelemetry::metrics::Meter;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::processor::AaveEventProcessor;

pub(crate) struct Stores {
    pub(crate) clickhouse: Client,
    pub(crate) postgres: postgres::Pool,
}

/// # Errors
///
/// [`chain::Error`] when no client can be built for the providers.
pub(crate) fn assemble(
    config: &Config,
    stores: Stores,
    meter: &Meter,
    status: Arc<IndexerStatus>,
    cancel: CancellationToken,
) -> Result<Pipeline, chain::Error> {
    let options = config.chain;
    let chain = Arc::new(JsonRpc::connect(&config.rpc, meter)?);
    let processor = |contract: Contract, address: Address| -> Box<dyn BlockProcessor> {
        Box::new(AaveEventProcessor::new(
            options.chain_id,
            contract,
            address,
            Arc::clone(&chain) as Arc<dyn LogReader>,
            Box::new(ClickHouseEventLedger::new(
                stores.clickhouse.clone(),
                contract,
            )),
        ))
    };

    Ok(Pipeline {
        processors: vec![
            processor(Contract::Spoke, config.spoke),
            processor(Contract::Hub, config.hub),
        ],
        reorg_detector: Box::new(HashChainReorgDetector::new(
            options.chain_id,
            config.finality_depth,
            Arc::clone(&chain) as _,
            Box::new(PostgresBlockHeaderStore::new(stores.postgres.clone())),
        )),
        cursor_store: Box::new(PostgresCursorStore::new(stores.postgres)),
        chain_client: chain,
        instruments: Instruments::new(
            meter,
            options.chain_id,
            options.max_range_size,
            Arc::clone(&status),
        ),
        status,
        cancel,
        options,
    })
}
