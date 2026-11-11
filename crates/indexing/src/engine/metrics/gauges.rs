//! Observable, read at collection: a stalled loop makes no transitions, so a pushed gauge would
//! freeze at its last healthy value exactly when someone looks.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use opentelemetry::KeyValue;
use opentelemetry::metrics::Meter;

use crate::engine::status::{IndexerSnapshot, IndexerState, IndexerStatus};

pub(crate) fn register(
    meter: &Meter,
    chain: &KeyValue,
    status: Arc<IndexerStatus>,
    range_size: Arc<AtomicU64>,
) {
    let gauges = Gauges {
        meter,
        chain: chain.clone(),
        snapshots: Arc::new(PerCollection::new(status)),
    };

    gauges.read(
        "indexer.cursor.block",
        "{block}",
        "How far the loop has got, durably.",
        |snapshot| snapshot.last_block,
    );
    gauges.read(
        "indexer.head.block",
        "{block}",
        "The highest head observed, clamped to a high-water mark.",
        |snapshot| snapshot.head,
    );
    gauges.read(
        "indexer.lag.blocks",
        "{block}",
        "Head minus cursor. The alert, and the number that had no way out before.",
        |snapshot| Some(snapshot.head?.saturating_sub(snapshot.last_block?)),
    );
    gauges.read(
        "indexer.consecutive_failures",
        "{failure}",
        "Sizes the backoff, so it also says how long until the next attempt.",
        |snapshot| Some(u64::from(snapshot.consecutive_failures)),
    );
    gauges.range_size(range_size);
    gauges.progress_age();
    gauges.state();
}

struct Gauges<'a> {
    meter: &'a Meter,
    chain: KeyValue,
    snapshots: Arc<PerCollection>,
}

impl Gauges<'_> {
    /// `None` publishes no point: a zero would draw a cliff at every restart and a huge lag.
    fn read(
        &self,
        name: &'static str,
        unit: &'static str,
        description: &'static str,
        read: fn(&IndexerSnapshot) -> Option<u64>,
    ) {
        let (snapshots, chain) = (Arc::clone(&self.snapshots), [self.chain.clone()]);

        self.meter
            .u64_observable_gauge(name)
            .with_unit(unit)
            .with_description(description)
            .with_callback(move |observer| {
                if let Some(value) = read(&snapshots.snapshot()) {
                    observer.observe(value, &chain);
                }
            })
            .build();
    }

    fn range_size(&self, range_size: Arc<AtomicU64>) {
        let chain = [self.chain.clone()];

        self.meter
            .u64_observable_gauge("indexer.range.size")
            .with_unit("{block}")
            .with_description(
                "Dispatch width, which only ever halves. A falling line is a provider degrading us.",
            )
            .with_callback(move |observer| {
                observer.observe(range_size.load(Ordering::Relaxed), &chain);
            })
            .build();
    }

    fn progress_age(&self) {
        let (snapshots, chain) = (Arc::clone(&self.snapshots), [self.chain.clone()]);

        self.meter
            .f64_observable_gauge("indexer.progress.age")
            .with_unit("s")
            .with_description(
                "Seconds since anything advanced. Graphable before it fails readiness.",
            )
            .with_callback(move |observer| {
                observer.observe(snapshots.snapshot().progress_age.as_secs_f64(), &chain);
            })
            .build();
    }

    fn state(&self) {
        let (snapshots, chain) = (Arc::clone(&self.snapshots), self.chain.clone());

        self.meter
            .u64_observable_gauge("indexer.state")
            .with_description(
                "One series per state, 1 or 0. Alert on failed without parsing a string.",
            )
            .with_callback(move |observer| {
                let current = snapshots.snapshot().state;

                for state in IndexerState::ALL {
                    observer.observe(
                        u64::from(current == state),
                        &[chain.clone(), KeyValue::new("state", state.as_str())],
                    );
                }
            })
            .build();
    }
}

/// The API has no batch callback, so the gauges share a read by time instead: a collection runs
/// its callbacks microseconds apart and scrapes come seconds apart. Read separately, the cursor,
/// head and lag of one scrape would come from three moments and disagree.
struct PerCollection {
    status: Arc<IndexerStatus>,
    held: Mutex<Option<(Instant, IndexerSnapshot)>>,
}

const ONE_COLLECTION: Duration = Duration::from_millis(1);

impl PerCollection {
    fn new(status: Arc<IndexerStatus>) -> Self {
        Self {
            status,
            held: Mutex::new(None),
        }
    }

    fn snapshot(&self) -> IndexerSnapshot {
        self.snapshot_at(Instant::now())
    }

    fn snapshot_at(&self, now: Instant) -> IndexerSnapshot {
        let mut held = self.held.lock().unwrap_or_else(|held| held.into_inner());

        if let Some((taken, snapshot)) = held.as_ref()
            && now.saturating_duration_since(*taken) < ONE_COLLECTION
        {
            return snapshot.clone();
        }

        let snapshot = self.status.snapshot();
        *held = Some((now, snapshot.clone()));

        snapshot
    }
}

#[cfg(test)]
mod tests {
    //! Here and not through a collection: the loop stands still during one flush, so every gauge
    //! would agree whether or not they shared a read.

    use std::sync::Arc;
    use std::time::Instant;

    use super::{ONE_COLLECTION, PerCollection};
    use crate::engine::status::IndexerStatus;

    #[test]
    fn every_gauge_in_one_collection_reads_the_same_snapshot() {
        let status = Arc::new(IndexerStatus::new());
        let gauges = PerCollection::new(Arc::clone(&status));
        status.progressed(100);
        let collection = Instant::now();

        let first = gauges.snapshot_at(collection);
        status.progressed(200);
        let second = gauges.snapshot_at(collection);

        assert_eq!(first.last_block, Some(100));
        assert_eq!(second.last_block, Some(100), "a gauge saw a later loop");
    }

    #[test]
    fn the_next_collection_reads_a_fresh_one() {
        let status = Arc::new(IndexerStatus::new());
        let gauges = PerCollection::new(Arc::clone(&status));
        status.progressed(100);
        let collection = Instant::now();
        let _ = gauges.snapshot_at(collection);

        status.progressed(200);

        assert_eq!(
            gauges.snapshot_at(collection + ONE_COLLECTION).last_block,
            Some(200)
        );
    }
}
