//! Real iterations over a migrated schema, a chain that answers from arithmetic, and processors that
//! record what reached them.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use alloy_primitives::{B256, U256};
use async_trait::async_trait;
use indexing::{
    BlockHeader, BlockHeaderStore as _, BlockProcessor, ChainClient, ChainError, Cursor,
    CursorStore as _, Failure, Indexer, Iteration, Options, Outcome, PostgresBlockHeaderStore,
    PostgresCursorStore,
};
use test_db::Scratch;
use tokio_util::sync::CancellationToken;
use units::ChainId;

use super::assemble;

const CHAIN_ID: ChainId = ChainId::known(1);
const HEAD: u64 = 112;
const FINALITY_DEPTH: u64 = 3;
const OPTIONS: Options = Options {
    chain_id: CHAIN_ID,
    start_block: 100,
    max_range_size: 10,
    poll_interval: Duration::ZERO,
};

/// Every block from `forked_at` up has a different hash from the one a chain without it gives.
#[derive(Default)]
struct FakeChain {
    forked_at: Option<u64>,
}

impl FakeChain {
    fn hash(&self, number: u64) -> B256 {
        let branch = u64::from(self.forked_at.is_some_and(|fork| number >= fork));
        B256::from(U256::from(number) + (U256::from(branch) << 64))
    }
}

#[async_trait]
impl ChainClient for FakeChain {
    async fn chain_id(&self) -> Result<ChainId, ChainError> {
        Ok(CHAIN_ID)
    }

    async fn head_block_number(&self) -> Result<u64, ChainError> {
        Ok(HEAD)
    }

    async fn block_header(&self, number: u64) -> Result<BlockHeader, ChainError> {
        Ok(BlockHeader {
            number,
            hash: self.hash(number),
            parent_hash: self.hash(number - 1),
            timestamp: number,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seen {
    Range(&'static str, u64, u64),
    Reorg(&'static str, u64, u64),
}

type Log = Arc<Mutex<Vec<Seen>>>;

struct RecordingProcessor {
    name: &'static str,
    seen: Log,
}

#[async_trait]
impl BlockProcessor for RecordingProcessor {
    fn name(&self) -> &str {
        self.name
    }

    async fn on_block_range(
        &self,
        from: u64,
        to: u64,
        _cancel: &CancellationToken,
    ) -> Result<Outcome, Failure> {
        let seen = Seen::Range(self.name, from, to);
        self.seen.lock().unwrap().push(seen);
        Ok(Outcome::Ok)
    }

    async fn on_reorg(
        &self,
        from: u64,
        to: u64,
        _cancel: &CancellationToken,
    ) -> Result<Outcome, Failure> {
        let seen = Seen::Reorg(self.name, from, to);
        self.seen.lock().unwrap().push(seen);
        Ok(Outcome::Ok)
    }
}

fn indexer(database: &Scratch, chain: FakeChain, names: &[&'static str]) -> (Indexer, Log) {
    let seen = Log::default();
    let processors = names
        .iter()
        .map(|&name| -> Box<dyn BlockProcessor> {
            Box::new(RecordingProcessor {
                name,
                seen: Arc::clone(&seen),
            })
        })
        .collect();
    let pipeline = assemble(
        OPTIONS,
        FINALITY_DEPTH,
        Arc::new(chain),
        database.pool().clone(),
        processors,
        &opentelemetry::global::meter("pipeline-test"),
    );

    (Indexer::new(pipeline), seen)
}

#[tokio::test]
async fn hands_each_range_to_every_processor_in_order_and_records_how_far_it_got() {
    let database = Scratch::migrated("rust_pipeline", "ranges").await;
    let (mut indexer, seen) = indexer(&database, FakeChain::default(), &["first", "second"]);

    for cursor_at in [109, 110] {
        assert_eq!(
            indexer.run_once().await,
            Iteration::Progressed { cursor_at }
        );
    }

    assert_eq!(
        *seen.lock().unwrap(),
        [
            Seen::Range("first", 100, 109),
            Seen::Range("second", 100, 109),
            Seen::Range("first", 110, 110),
            Seen::Range("second", 110, 110),
        ]
    );

    let window = PostgresBlockHeaderStore::new(database.pool().clone());
    let retained = window.load(CHAIN_ID).await.unwrap();
    assert_eq!(retained.iter().map(|header| header.number).max(), Some(110));

    let cursors = PostgresCursorStore::new(database.pool().clone());
    assert_eq!(
        cursors.load(CHAIN_ID).await.unwrap(),
        Some(Cursor {
            chain_id: CHAIN_ID,
            last_block: 110,
            last_hash: FakeChain::default().hash(110),
        })
    );
}

#[tokio::test]
async fn finds_a_fork_that_happened_while_it_was_stopped() {
    let database = Scratch::migrated("rust_pipeline", "restart").await;
    let (mut before, _) = indexer(&database, FakeChain::default(), &["only"]);
    for _ in 0..3 {
        assert!(matches!(
            before.run_once().await,
            Iteration::Progressed { .. }
        ));
    }

    let forked = FakeChain {
        forked_at: Some(111),
    };
    let (mut after, seen) = indexer(&database, forked, &["only"]);
    for cursor_at in [110, 111] {
        assert_eq!(after.run_once().await, Iteration::Progressed { cursor_at });
    }

    assert_eq!(
        *seen.lock().unwrap(),
        [Seen::Reorg("only", 111, 111), Seen::Range("only", 111, 111)]
    );
}
