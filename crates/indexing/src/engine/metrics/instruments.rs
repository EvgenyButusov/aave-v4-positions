use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use opentelemetry::metrics::{Counter, Histogram, Meter};
use opentelemetry::{KeyValue, Value};
use tracing::warn;
use units::ChainId;

use crate::engine::Iteration;
use crate::engine::dispatch::Operation;
use crate::engine::metrics::gauges;
use crate::engine::status::IndexerStatus;

/// Takes a [`Meter`] rather than a scope, so this crate does not link the telemetry crate.
#[derive(Clone)]
pub struct Instruments {
    chain: KeyValue,
    iterations: Counter<u64>,
    iteration_duration: Histogram<f64>,
    processor_duration: Histogram<f64>,
    reorg_depth: Histogram<u64>,
    blocks_indexed: Counter<u64>,
    range_size: Arc<AtomicU64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Bootstrap,
    Inspect,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Bootstrap => "bootstrap",
            Self::Inspect => "inspect",
        }
    }
}

impl Instruments {
    #[must_use]
    pub fn new(
        meter: &Meter,
        chain_id: ChainId,
        max_range_size: u64,
        status: Arc<IndexerStatus>,
    ) -> Self {
        let chain = KeyValue::new("chain.id", Value::I64(i64::from(chain_id)));
        let range_size = Arc::new(AtomicU64::new(max_range_size));
        gauges::register(meter, &chain, status, Arc::clone(&range_size));

        Self {
            chain,
            iterations: meter
                .u64_counter("indexer.iterations")
                .with_description("Loop iterations by outcome. The retry rate, long before the stall alarm.")
                .build(),
            iteration_duration: meter
                .f64_histogram("indexer.iteration.duration")
                .with_unit("s")
                .with_description("How long one pass took, so a slow iteration is distinguishable from a frequent one.")
                .build(),
            processor_duration: meter
                .f64_histogram("indexer.processor.duration")
                .with_unit("s")
                .with_description("Per processor. Dispatch is sequential, so one slow processor is the whole loop.")
                .build(),
            reorg_depth: meter
                .u64_histogram("indexer.reorg.depth")
                .with_unit("{block}")
                .with_description("How deep each reorg reached; its count is how often. Watch it against the finality depth.")
                .build(),
            blocks_indexed: meter
                .u64_counter("indexer.blocks.indexed")
                .with_unit("{block}")
                .with_description("Throughput. Its rate over the lag gauge is the catch-up estimate.")
                .build(),
            range_size,
        }
    }

    pub(crate) fn iteration(&self, result: &Iteration, elapsed: Duration) {
        let attributes = [
            self.chain.clone(),
            KeyValue::new("outcome", result.outcome()),
        ];

        self.iterations.add(1, &attributes);
        self.iteration_duration
            .record(elapsed.as_secs_f64(), &attributes);
    }

    pub(crate) fn processor(
        &self,
        name: &str,
        operation: Operation,
        outcome: &'static str,
        elapsed: Duration,
    ) {
        self.processor_duration.record(
            elapsed.as_secs_f64(),
            &[
                self.chain.clone(),
                KeyValue::new("processor", name.to_owned()),
                KeyValue::new("operation", operation.as_str()),
                KeyValue::new("outcome", outcome),
            ],
        );
    }

    /// An event on the iteration's span rather than a span of its own: a fork happens inside one.
    pub(crate) fn reorg(
        &self,
        first_invalid_block: u64,
        last_invalid_block: u64,
        phase: Phase,
        at_block: Option<u64>,
    ) {
        let depth = last_invalid_block - first_invalid_block + 1;
        let phase = phase.as_str();

        self.reorg_depth
            .record(depth, &[self.chain.clone(), KeyValue::new("phase", phase)]);

        // A message renames the span event after itself, so the queryable `reorg` and the line are two.
        tracing::event!(
            name: "reorg",
            tracing::Level::WARN,
            depth,
            phase,
            first_invalid_block,
            last_invalid_block,
        );
        warn!(
            depth,
            phase, first_invalid_block, last_invalid_block, at_block, "reorg; blocks are stale"
        );
    }

    pub(crate) fn indexed(&self, blocks: u64) {
        self.blocks_indexed
            .add(blocks, std::slice::from_ref(&self.chain));
    }

    pub(crate) fn narrowed_to(&self, size: u64) {
        self.range_size.store(size, Ordering::Relaxed);
    }
}
