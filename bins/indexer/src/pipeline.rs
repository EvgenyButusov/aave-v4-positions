//! The loop, with its cursor and header window in Postgres, over whatever chain and processors it
//! is given.

#[cfg(test)]
mod tests;

use std::sync::Arc;

use indexing::{
    BlockProcessor, ChainClient, HashChainReorgDetector, IndexerStatus, Instruments, Options,
    Pipeline, PostgresBlockHeaderStore, PostgresCursorStore,
};
use opentelemetry::metrics::Meter;
use tokio_util::sync::CancellationToken;

pub(crate) fn assemble(
    options: Options,
    finality_depth: u64,
    chain_client: Arc<dyn ChainClient>,
    postgres: postgres::Pool,
    processors: Vec<Box<dyn BlockProcessor>>,
    meter: &Meter,
) -> Pipeline {
    let status = Arc::new(IndexerStatus::new());

    Pipeline {
        reorg_detector: Box::new(HashChainReorgDetector::new(
            options.chain_id,
            finality_depth,
            Arc::clone(&chain_client),
            Box::new(PostgresBlockHeaderStore::new(postgres.clone())),
        )),
        cursor_store: Box::new(PostgresCursorStore::new(postgres)),
        instruments: Instruments::new(
            meter,
            options.chain_id,
            options.max_range_size,
            Arc::clone(&status),
        ),
        options,
        chain_client,
        processors,
        status,
        cancel: CancellationToken::new(),
    }
}
