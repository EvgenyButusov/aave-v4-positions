use std::collections::BTreeSet;
use std::time::Duration;

use opentelemetry::KeyValue;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, Metric, MetricData, ResourceMetrics};

use crate::engine::harness::{
    Fixture, HEAD, MAX_RANGE, Setup, failed, fixture, narrowing, plain, retry,
};
use crate::fixtures::hash_of;
use crate::reorg::Verdict;

/// Resets first: the exporter accumulates every flush, so a second read would see the first's points.
fn read<T>(fixture: &Fixture, keep: impl Fn(&Metric) -> Vec<T>) -> Vec<T> {
    fixture.exporter.reset();
    fixture.provider.force_flush().expect("a collection");
    let batches = fixture
        .exporter
        .get_finished_metrics()
        .expect("the exported batches");

    batches
        .iter()
        .flat_map(ResourceMetrics::scope_metrics)
        .flat_map(|scope| scope.metrics())
        .flat_map(keep)
        .collect()
}

fn instrument_names(fixture: &Fixture) -> BTreeSet<String> {
    read(fixture, |metric| vec![metric.name().to_owned()])
        .into_iter()
        .collect()
}

fn labels<'a>(attributes: impl Iterator<Item = &'a KeyValue>) -> Vec<String> {
    attributes
        .map(|attribute| format!("{}={}", attribute.key, attribute.value.as_str()))
        .collect()
}

fn points(fixture: &Fixture, name: &str) -> Vec<(Vec<String>, u64)> {
    read(fixture, |metric| match metric.data() {
        AggregatedMetrics::U64(MetricData::Gauge(gauge)) if metric.name() == name => gauge
            .data_points()
            .map(|point| (labels(point.attributes()), point.value()))
            .collect(),
        AggregatedMetrics::U64(MetricData::Sum(sum)) if metric.name() == name => sum
            .data_points()
            .map(|point| (labels(point.attributes()), point.value()))
            .collect(),
        AggregatedMetrics::U64(MetricData::Histogram(histogram)) if metric.name() == name => {
            histogram
                .data_points()
                .map(|point| (labels(point.attributes()), point.sum()))
                .collect()
        }
        _ => Vec::new(),
    })
}

fn seconds(fixture: &Fixture, name: &str) -> Vec<(Vec<String>, f64)> {
    read(fixture, |metric| match metric.data() {
        AggregatedMetrics::F64(MetricData::Gauge(gauge)) if metric.name() == name => gauge
            .data_points()
            .map(|point| (labels(point.attributes()), point.value()))
            .collect(),
        AggregatedMetrics::F64(MetricData::Histogram(histogram)) if metric.name() == name => {
            histogram
                .data_points()
                .map(|point| (labels(point.attributes()), point.sum()))
                .collect()
        }
        _ => Vec::new(),
    })
}

/// Recorded into first: a histogram with no data point is absent from a collection, not empty.
#[tokio::test]
async fn names_every_instrument_the_engine_records() {
    let mut fixture = plain();
    fixture.reorg_detector.queue([Verdict::Reorg {
        first_invalid_block: 98,
        last_invalid_block: 100,
        last_valid_hash: hash_of(b'a', 97),
    }]);

    let _ = fixture.indexer.run_once().await;
    let _ = fixture.indexer.run_once().await;

    assert_eq!(
        instrument_names(&fixture),
        BTreeSet::from(
            [
                "indexer.iterations",
                "indexer.iteration.duration",
                "indexer.processor.duration",
                "indexer.reorg.depth",
                "indexer.blocks.indexed",
                "indexer.cursor.block",
                "indexer.head.block",
                "indexer.lag.blocks",
                "indexer.progress.age",
                "indexer.consecutive_failures",
                "indexer.state",
                "indexer.range.size",
            ]
            .map(str::to_owned)
        )
    );
}

#[tokio::test]
async fn an_unindexed_chain_publishes_no_cursor_point_rather_than_a_zero() {
    let fixture = plain();

    assert!(points(&fixture, "indexer.cursor.block").is_empty());
    assert!(points(&fixture, "indexer.lag.blocks").is_empty());
    // The ones that always have an answer are still there.
    assert_eq!(points(&fixture, "indexer.consecutive_failures").len(), 1);
}

#[tokio::test]
async fn the_lag_is_the_head_minus_the_cursor() {
    let mut fixture = plain();
    fixture.reorg_detector.settled_through(HEAD);
    let _ = fixture.indexer.run_once().await;

    let cursor = points(&fixture, "indexer.cursor.block")[0].1;
    let head = points(&fixture, "indexer.head.block")[0].1;
    let lag = points(&fixture, "indexer.lag.blocks")[0].1;

    assert_eq!(lag, head - cursor);
}

#[tokio::test]
async fn publishes_one_series_per_state() {
    let mut fixture = plain();
    fixture.processors[0].queue([failed("unfixable")]);
    let _ = fixture.indexer.run_once().await;

    let series = points(&fixture, "indexer.state");
    let hot: Vec<_> = series
        .iter()
        .filter(|(_, value)| *value == 1)
        .flat_map(|(attributes, _)| attributes.clone())
        .filter(|attribute| attribute.starts_with("state="))
        .collect();

    assert_eq!(series.len(), 5, "{series:?}");
    assert_eq!(hot, vec!["state=failed".to_owned()]);
}

#[tokio::test]
async fn the_range_gauge_follows_a_narrowing() {
    let mut fixture = plain();
    fixture.reorg_detector.settled_through(HEAD);
    fixture.processors[0].queue([narrowing("too wide")]);

    assert_eq!(points(&fixture, "indexer.range.size")[0].1, MAX_RANGE);
    let _ = fixture.indexer.run_once().await;

    assert_eq!(points(&fixture, "indexer.range.size")[0].1, MAX_RANGE / 2);
}

#[tokio::test]
async fn counts_the_blocks_it_indexed() {
    let mut fixture = plain();
    fixture.reorg_detector.settled_through(HEAD);

    let _ = fixture.indexer.run_once().await;

    assert_eq!(points(&fixture, "indexer.blocks.indexed")[0].1, MAX_RANGE);
}

#[tokio::test]
async fn counts_an_iteration_under_its_outcome() {
    let mut fixture = plain();
    fixture.processors[0].queue([retry("busy")]);

    let _ = fixture.indexer.run_once().await;

    let counted = points(&fixture, "indexer.iterations");
    assert!(
        counted.iter().any(
            |(attributes, value)| attributes.contains(&"outcome=retry".to_owned()) && *value == 1
        ),
        "{counted:?}"
    );
}

#[tokio::test]
async fn records_how_deep_a_reorg_reached_and_who_saw_it() {
    let mut fixture = plain();
    fixture.reorg_detector.queue([Verdict::Reorg {
        first_invalid_block: 98,
        last_invalid_block: 100,
        last_valid_hash: hash_of(b'a', 97),
    }]);

    let _ = fixture.indexer.run_once().await;

    let (labels, depth) = &points(&fixture, "indexer.reorg.depth")[0];
    assert_eq!(*depth, 3);
    assert!(labels.contains(&"phase=inspect".to_owned()), "{labels:?}");
}

#[tokio::test]
async fn times_each_iteration() {
    let mut fixture = plain();

    let _ = fixture.indexer.run_once().await;

    let seconds = seconds(&fixture, "indexer.iteration.duration");
    assert_eq!(seconds.len(), 1, "{seconds:?}");
    assert!(seconds[0].1 > 0.0, "{seconds:?}");
}

#[tokio::test]
async fn times_each_processor_under_its_name_and_answer() {
    let mut fixture = fixture(Setup {
        processors: 2,
        ..Setup::default()
    });
    fixture.processors[1].queue([retry("busy")]);

    let _ = fixture.indexer.run_once().await;

    let timed: BTreeSet<_> = seconds(&fixture, "indexer.processor.duration")
        .into_iter()
        .map(|(mut labels, _)| {
            labels.retain(|label| !label.starts_with("chain.id"));
            labels.sort();
            labels.join(" ")
        })
        .collect();
    assert_eq!(
        timed,
        BTreeSet::from([
            "operation=block-range outcome=ok processor=p1".to_owned(),
            "operation=block-range outcome=retry processor=p2".to_owned(),
        ])
    );
}

// Collected on the exporter's thread, outside the runtime, so a paused clock never reaches it.
#[tokio::test]
async fn ages_while_nothing_advances() {
    let fixture = plain();

    std::thread::sleep(Duration::from_millis(20));

    let age = seconds(&fixture, "indexer.progress.age")[0].1;
    assert!(age >= 0.02, "{age}");
}
