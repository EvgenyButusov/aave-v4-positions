//! What an operator reads, captured from the formatted log.

use std::io::Write;
use std::sync::{Arc, Mutex};

use tracing::instrument::WithSubscriber as _;
use tracing_subscriber::layer::SubscriberExt as _;

use crate::engine::Indexer;
use crate::engine::harness::{failed, interesting, plain};

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .extend_from_slice(bytes);

        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

async fn logged(indexer: &mut Indexer) -> Vec<String> {
    interesting();

    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .without_time()
            .with_writer(move || writer.clone()),
    );

    indexer.run().with_subscriber(subscriber).await;

    let bytes = captured
        .0
        .lock()
        .unwrap_or_else(|held| held.into_inner())
        .clone();
    String::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn line<'a>(lines: &'a [String], message: &str) -> Vec<&'a String> {
    lines.iter().filter(|line| line.contains(message)).collect()
}

#[tokio::test]
async fn says_which_chain_it_started_on() {
    let mut fixture = plain();
    fixture.cancel.cancel();

    let lines = logged(&mut fixture.indexer).await;

    let started = line(&lines, "indexing started");
    assert_eq!(started.len(), 1, "{lines:?}");
    assert!(started[0].contains("INFO"), "{started:?}");
    assert!(started[0].contains("chain_id=1"), "{started:?}");
    assert!(started[0].contains("processors=1"), "{started:?}");
}

#[tokio::test(start_paused = true)]
async fn warns_on_every_retry_with_its_attempt() {
    let mut fixture = plain();
    fixture.processors[0].refuse_with("provider down");
    fixture.processors[0].cancel_on_call(2, fixture.cancel.clone());

    let lines = logged(&mut fixture.indexer).await;

    let warned = line(&lines, "iteration failed");
    assert_eq!(warned.len(), 2, "{lines:?}");
    for (attempt, warning) in warned.iter().enumerate() {
        assert!(warning.contains("WARN"), "{warning}");
        assert!(
            warning.contains(&format!("attempt={}", attempt + 1)),
            "{warning}"
        );
        assert!(warning.contains("provider down"), "{warning}");
    }
}

#[tokio::test]
async fn says_why_it_stopped() {
    let mut fixture = plain();
    fixture.processors[0].queue([failed("unfixable")]);

    let lines = logged(&mut fixture.indexer).await;

    let stopped = line(&lines, "indexing stopped");
    assert_eq!(stopped.len(), 1, "{lines:?}");
    assert!(stopped[0].contains("ERROR"), "{stopped:?}");
    assert!(stopped[0].contains("p1: unfixable"), "{stopped:?}");
}
