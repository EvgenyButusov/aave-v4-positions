//! Where the indexer has got to, and the loop that gets it there.
//!
//! Whatever touches the chain is a port with no adapter here: one would link a
//! provider and an HTTP transport, and `./scripts/binary-boundaries.sh` fails
//! if either reaches `bins/api`, which links this crate for [`SyncStatusStore`].

mod chain;
mod cursor;
mod engine;
#[cfg(test)]
mod fixtures;
mod processors;
mod reorg;

pub use chain::{
    Absence, BlockHeader, ChainClient, Erc20MetadataReader, Error as ChainError, Field, LogFilter,
    LogReader, RawLog, TokenMetadata,
};
pub use cursor::{
    Cursor, CursorStore, Error, PostgresCursorStore, PostgresSyncStatusStore, SyncStatus,
    SyncStatusStore,
};
pub use engine::{
    Indexer, IndexerSnapshot, IndexerState, IndexerStatus, Instruments, Iteration, Options,
    Pipeline, SHUTDOWN_DRAIN,
};
pub use processors::{BlockProcessor, Failure, Outcome};
pub use reorg::{
    BlockHeaderStore, Error as ReorgError, HashChainReorgDetector, PostgresBlockHeaderStore,
    ReorgDetector, Verdict,
};
