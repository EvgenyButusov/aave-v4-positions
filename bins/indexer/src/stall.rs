//! Whether the loop is still getting anywhere, as one row of the readiness report.
//!
//! Failing readiness drains nothing from a process that serves only probes, so this is an
//! alertable signal with no automatic recovery: the transition lines are what a human acts on.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use indexing::{IndexerSnapshot, IndexerState, IndexerStatus};
use ops::{CheckResult, CheckStatus};

pub(crate) struct Stall {
    status: Arc<IndexerStatus>,
    threshold: Duration,
    /// So a stall is logged when it starts and when it ends, not on every probe.
    stalled: AtomicBool,
}

impl Stall {
    pub(crate) fn new(status: Arc<IndexerStatus>, threshold: Duration) -> Self {
        Self {
            status,
            threshold,
            stalled: AtomicBool::new(false),
        }
    }

    /// Not through [`ops::check`], which warns on every failing probe.
    pub(crate) fn check(&self) -> CheckResult {
        let snapshot = self.status.snapshot();

        match self.judge(&snapshot) {
            Ok(()) => CheckResult {
                name: "indexer",
                status: CheckStatus::Up,
                error: None,
            },
            Err(error) => CheckResult {
                name: "indexer",
                status: CheckStatus::Down,
                error: Some(error),
            },
        }
    }

    fn judge(&self, snapshot: &IndexerSnapshot) -> Result<(), String> {
        let IndexerSnapshot {
            state,
            last_block,
            head,
            progress_age,
            ..
        } = snapshot;

        match state {
            IndexerState::Failed => {
                let reason = snapshot.reason.as_deref().unwrap_or("unknown reason");
                return Err(format!("indexing stopped: {reason}"));
            }
            // Nothing to judge yet, and down here would stop the pod ever becoming ready.
            IndexerState::Starting => return Ok(()),
            IndexerState::Running | IndexerState::Retrying | IndexerState::Stopped => {}
        }

        if *progress_age <= self.threshold {
            if self.stalled.swap(false, Ordering::Relaxed) {
                tracing::info!(last_block, head, "indexer is making progress again");
            }
            return Ok(());
        }

        let seconds = (progress_age.as_millis() + 500) / 1_000;
        if !self.stalled.swap(true, Ordering::Relaxed) {
            tracing::error!(
                stalled_for_seconds = seconds,
                last_block,
                head,
                "indexer has stopped making progress"
            );
        }

        let block = last_block.map_or_else(|| "none".to_owned(), |block| block.to_string());
        let head = head.map_or_else(|| "unknown".to_owned(), |head| head.to_string());
        Err(format!(
            "no progress for {seconds}s at block {block} (head {head})"
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::Mutex;

    use tracing::instrument::WithSubscriber as _;

    use super::*;

    const THRESHOLD: Duration = Duration::from_secs(300);

    fn stall() -> (Arc<IndexerStatus>, Stall) {
        let status = Arc::new(IndexerStatus::new());

        (Arc::clone(&status), Stall::new(status, THRESHOLD))
    }

    fn down(check: &CheckResult) -> &str {
        assert_eq!(check.status, CheckStatus::Down, "{check:?}");
        check.error.as_deref().unwrap_or_default()
    }

    #[tokio::test(start_paused = true)]
    async fn a_loop_still_starting_is_ready_however_long_it_has_been() {
        let (_, stall) = stall();
        tokio::time::advance(THRESHOLD * 10).await;

        assert_eq!(stall.check().status, CheckStatus::Up);
    }

    #[tokio::test(start_paused = true)]
    async fn a_loop_that_stopped_is_down_and_says_why_before_the_threshold() {
        let (status, stall) = stall();
        status.failed("chain 5 is not chain 1".to_owned());

        assert_eq!(
            down(&stall.check()),
            "indexing stopped: chain 5 is not chain 1"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_threshold_itself_is_still_progress() {
        let (status, stall) = stall();
        status.progressed(100);
        tokio::time::advance(THRESHOLD).await;

        assert_eq!(stall.check().status, CheckStatus::Up);
    }

    #[tokio::test(start_paused = true)]
    async fn past_the_threshold_it_names_the_rounded_wait_the_block_and_the_head() {
        let (status, stall) = stall();
        status.observe_head(120);
        status.progressed(100);
        tokio::time::advance(THRESHOLD + Duration::from_millis(500)).await;

        assert_eq!(
            down(&stall.check()),
            "no progress for 301s at block 100 (head 120)"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_loop_that_never_read_the_chain_has_no_block_and_no_head() {
        let (status, stall) = stall();
        status.retried("provider unreachable".to_owned());
        tokio::time::advance(THRESHOLD * 2).await;

        assert_eq!(
            down(&stall.check()),
            "no progress for 600s at block none (head unknown)"
        );
    }

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

    #[tokio::test(start_paused = true)]
    async fn logs_a_stall_once_when_it_starts_and_once_when_it_ends() {
        test_tracing::interesting();
        let (status, stall) = stall();
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(move || writer.clone())
            .finish();

        async {
            status.progressed(100);
            tokio::time::advance(THRESHOLD * 2).await;
            for _ in 0..3 {
                stall.check();
            }
            status.progressed(101);
            for _ in 0..3 {
                stall.check();
            }
        }
        .with_subscriber(subscriber)
        .await;

        let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines.len(), 2, "{log}");
        assert!(
            lines[0].starts_with("ERROR")
                && lines[0].contains("indexer has stopped making progress")
                && lines[0].contains("stalled_for_seconds=600 last_block=100"),
            "{log}"
        );
        assert!(
            lines[1].starts_with(" INFO")
                && lines[1].contains("indexer is making progress again last_block=101"),
            "{log}"
        );
    }
}
