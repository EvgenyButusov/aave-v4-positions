#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::fmt::Display;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use alloy_primitives::Address;
use async_trait::async_trait;
use futures_util::{StreamExt as _, stream};
use indexing::{
    BlockProcessor, ChainClient, Erc20MetadataReader, Failure, Field, Outcome, TokenMetadata,
};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::instrument::WithSubscriber as _;
use units::ChainId;

use crate::{
    EnrichmentSettings, PendingTokens, TokenListings, TokenMetadataRow, TokenMetadataStore,
};

/// Fills in what each listed token calls itself, without the loop ever waiting on an ERC-20.
///
/// A dispatch only decides whether to start a run, and returns before the run does anything: the
/// reads go to third-party contracts over a provider that may be slow or down, and the ledgers
/// must not queue behind them. At most one run is in flight, and one that leaves a gap open backs
/// off before the next.
///
/// What to read is the gap between the Hub's listings and the store, so a skipped, interrupted or
/// lost run costs nothing: the next one finds the same gap. The usual dispatch asks no one, because
/// the Hub processor hands each new listing to [`PendingTokens`] as it writes it. The whole set is
/// asked for only while a full check is owed: at start, since every `AddAsset` on mainnet is behind
/// any live cursor, and after a run that did not finish, since what it drained is not pushed again.
pub struct TokenEnrichmentProcessor(Arc<Enrichment>);

struct Enrichment {
    chain_id: ChainId,
    settings: EnrichmentSettings,
    listings: Box<dyn TokenListings>,
    pending: Arc<PendingTokens>,
    store: Box<dyn TokenMetadataStore>,
    reader: Arc<dyn Erc20MetadataReader>,
    chain: Arc<dyn ChainClient>,
    state: Mutex<State>,
}

/// A run starts owing a full check and backing off, and only one that closes every gap clears
/// both: a run that panics after draining is then retried, after the delay, against the whole set.
struct State {
    run: Option<JoinHandle<()>>,
    retry_after: Option<Instant>,
    full_check_owed: bool,
}

impl TokenEnrichmentProcessor {
    #[must_use]
    pub fn new(
        chain_id: ChainId,
        settings: EnrichmentSettings,
        listings: Box<dyn TokenListings>,
        pending: Arc<PendingTokens>,
        store: Box<dyn TokenMetadataStore>,
        reader: Arc<dyn Erc20MetadataReader>,
        chain: Arc<dyn ChainClient>,
    ) -> Self {
        Self(Arc::new(Enrichment {
            chain_id,
            settings,
            listings,
            pending,
            store,
            reader,
            chain,
            state: Mutex::new(State {
                run: None,
                retry_after: None,
                full_check_owed: true,
            }),
        }))
    }

    /// Never awaits: the run it starts is spawned, and outlives the dispatch.
    fn wake(&self, cancel: &CancellationToken) {
        let mut state = self.0.state();
        let owed = state.full_check_owed || !self.0.pending.is_empty();
        let idle = state.run.as_ref().is_none_or(JoinHandle::is_finished);
        let due = state.retry_after.is_none_or(|at| Instant::now() >= at);
        if !(owed && idle && due) || cancel.is_cancelled() {
            return;
        }

        let full = std::mem::replace(&mut state.full_check_owed, true);
        state.retry_after = Some(Instant::now() + self.0.settings.retry_delay);
        let (enrichment, cancel) = (Arc::clone(&self.0), cancel.clone());
        state.run = Some(tokio::spawn(
            async move { enrichment.run(full, &cancel).await }.with_current_subscriber(),
        ));
    }

    /// Put back once finished, so a case still sees the handle the next dispatch will.
    #[cfg(test)]
    async fn settled(&self) {
        let run = self.0.state().run.take();
        if let Some(mut run) = run {
            if !run.is_finished() {
                let _ = (&mut run).await;
            }
            self.0.state().run.get_or_insert(run);
        }
    }
}

#[async_trait]
impl BlockProcessor for TokenEnrichmentProcessor {
    fn name(&self) -> &str {
        "token-enrichment"
    }

    async fn on_block_range(
        &self,
        _from: u64,
        _to: u64,
        cancel: &CancellationToken,
    ) -> Result<Outcome, Failure> {
        self.wake(cancel);
        Ok(Outcome::Ok)
    }

    /// A fork does not unmint an ERC-20; a rolled-back listing leaves a row nothing joins to.
    async fn on_reorg(
        &self,
        _from: u64,
        _to: u64,
        _cancel: &CancellationToken,
    ) -> Result<Outcome, Failure> {
        Ok(Outcome::Ok)
    }
}

impl Enrichment {
    async fn run(&self, full: bool, cancel: &CancellationToken) {
        let missing = match self.missing(full).await {
            Ok(missing) => missing,
            Err(refused) => return self.back_off(&refused),
        };
        if missing.is_empty() {
            return self.caught_up();
        }
        if cancel.is_cancelled() {
            return;
        }

        match self.enrich(&missing, cancel).await {
            Ok(0) => self.caught_up(),
            Ok(open) => self.back_off(&format!("{open} token(s) still unread")),
            Err(refused) => self.back_off(&refused),
        }
    }

    /// Listed on the Hub and absent from the store. Drained even when the whole set is asked for,
    /// or what it held would wake the next dispatch for work this run covers.
    async fn missing(&self, full: bool) -> Result<Vec<Address>, crate::Error> {
        let pushed = self.pending.drain();
        let listed = if full {
            self.listings.all(self.chain_id).await?
        } else {
            pushed
        };
        if listed.is_empty() {
            return Ok(Vec::new());
        }

        let known = self.store.labels(self.chain_id).await?;
        let missing: BTreeSet<Address> = listed
            .into_iter()
            .filter(|token| !known.contains_key(token))
            .collect();
        Ok(missing.into_iter().collect())
    }

    /// Reads at the head, never at the dispatched range: a backfill's range is history a full node
    /// cannot serve, and metadata does not change. Returns how many gaps are still open.
    async fn enrich(
        &self,
        tokens: &[Address],
        cancel: &CancellationToken,
    ) -> Result<usize, Failure> {
        let head = self.chain.head_block_number().await?;

        let reads: Vec<(Address, TokenMetadata)> = stream::iter(tokens.iter().copied())
            .map(|token| async move { (token, self.reader.read(token, head).await) })
            .buffer_unordered(self.settings.concurrency)
            .collect()
            .await;

        let rows: Vec<TokenMetadataRow> = reads
            .into_iter()
            .filter_map(|(token, metadata)| self.row(token, &metadata, head))
            .collect();

        // Written even on the way out: the reads are paid for.
        self.store.put(&rows).await?;
        if !rows.is_empty() {
            tracing::info!(tokens = rows.len(), block = head, "enriched tokens");
        }
        if cancel.is_cancelled() {
            tracing::info!("shutting down; enrichment resumes from the gap");
        }
        Ok(tokens.len() - rows.len())
    }

    /// A token that answered is stored even with every field absent, so it is not read again; one
    /// the call never reached is not, so the gap stays open.
    fn row(&self, token: Address, metadata: &TokenMetadata, head: u64) -> Option<TokenMetadataRow> {
        if !metadata.answered() {
            tracing::warn!(
                token = %token,
                symbol = unreached(&metadata.symbol),
                name = unreached(&metadata.name),
                decimals = unreached(&metadata.decimals),
                "token unreached; the gap stays open"
            );
            return None;
        }

        Some(TokenMetadataRow {
            chain_id: self.chain_id,
            token,
            symbol: metadata.symbol.value().cloned(),
            name: metadata.name.value().cloned(),
            token_decimals: metadata.decimals.value().copied(),
            fetched_at_block: head,
        })
    }

    fn caught_up(&self) {
        let mut state = self.state();
        state.full_check_owed = false;
        state.retry_after = None;
    }

    /// The top-level message only: a transport error's source carries the provider's URL.
    fn back_off(&self, reason: &dyn Display) {
        let mut state = self.state();
        state.full_check_owed = true;
        state.retry_after = Some(Instant::now() + self.settings.retry_delay);
        tracing::warn!(
            reason = %reason,
            retry_in = ?self.settings.retry_delay,
            "enrichment incomplete"
        );
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn unreached<T>(field: &Field<T>) -> Option<&str> {
    match field {
        Field::Unreached(reason) => Some(reason),
        Field::Value(_) | Field::Absent(_) => None,
    }
}
