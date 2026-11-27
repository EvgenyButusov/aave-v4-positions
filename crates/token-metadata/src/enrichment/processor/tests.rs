use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alloy_primitives::{Address, address};
use async_trait::async_trait;
use futures_util::FutureExt as _;
use indexing::{
    Absence, BlockHeader, BlockProcessor as _, ChainClient, ChainError, Erc20MetadataReader, Field,
    Outcome, TokenMetadata,
};
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::instrument::WithSubscriber as _;
use tracing_subscriber::layer::SubscriberExt as _;
use units::ChainId;

use super::TokenEnrichmentProcessor;
use crate::memory::InMemoryStore;
use crate::{EnrichmentSettings, Error, PendingTokens, TokenListings, TokenMetadataRow};

const CHAIN_ID: ChainId = ChainId::known(1);
const HEAD: u64 = 25_652_782;
const RETRY: Duration = Duration::from_secs(60);

const USDC: Address = address!("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");
const WETH: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");

fn usdc() -> TokenMetadata {
    TokenMetadata {
        symbol: Field::Value("USDC".to_owned()),
        name: Field::Value("USD Coin".to_owned()),
        decimals: Field::Value(6),
    }
}

#[derive(Default)]
struct Listings {
    listed: Mutex<Vec<Address>>,
    calls: AtomicUsize,
    stalled: AtomicBool,
}

#[async_trait]
impl TokenListings for Arc<Listings> {
    async fn all(&self, _chain_id: ChainId) -> Result<Vec<Address>, Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.stalled.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        Ok(self.listed.lock().unwrap().clone())
    }
}

#[async_trait]
impl crate::TokenMetadataStore for Arc<InMemoryStore> {
    async fn labels(
        &self,
        chain_id: ChainId,
    ) -> Result<HashMap<Address, crate::TokenLabel>, Error> {
        self.as_ref().labels(chain_id).await
    }

    async fn put(&self, rows: &[TokenMetadataRow]) -> Result<(), Error> {
        self.as_ref().put(rows).await
    }
}

/// Answers `usdc()` for any token not given its own answer.
struct Reader {
    answers: Mutex<HashMap<Address, TokenMetadata>>,
    seen: Mutex<Vec<(Address, u64)>>,
    /// Holds every read while set, until `release`.
    gated: AtomicBool,
    gate: Semaphore,
    entered: Notify,
    panics: AtomicBool,
    in_flight: AtomicUsize,
    peak: AtomicUsize,
}

impl Default for Reader {
    fn default() -> Self {
        Self {
            answers: Mutex::default(),
            seen: Mutex::default(),
            gated: AtomicBool::new(false),
            gate: Semaphore::new(0),
            entered: Notify::new(),
            panics: AtomicBool::new(false),
            in_flight: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        }
    }
}

impl Reader {
    fn answer(&self, token: Address, metadata: TokenMetadata) {
        self.answers.lock().unwrap().insert(token, metadata);
    }

    fn release(&self) {
        self.gated.store(false, Ordering::SeqCst);
        self.gate.add_permits(Semaphore::MAX_PERMITS / 2);
    }

    fn reads(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

#[async_trait]
impl Erc20MetadataReader for Reader {
    async fn read(&self, token: Address, at_block: u64) -> TokenMetadata {
        self.seen.lock().unwrap().push((token, at_block));
        let in_flight = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(in_flight, Ordering::SeqCst);
        self.entered.notify_one();

        if self.gated.load(Ordering::SeqCst) {
            self.gate.acquire().await.unwrap().forget();
        }
        // Suspends, so a bound on how many run at once has something to bound.
        for _ in 0..3 {
            tokio::task::yield_now().await;
        }
        self.in_flight.fetch_sub(1, Ordering::SeqCst);

        assert!(!self.panics.load(Ordering::SeqCst), "the reader panicked");
        self.answers
            .lock()
            .unwrap()
            .get(&token)
            .cloned()
            .unwrap_or_else(usdc)
    }
}

#[derive(Default)]
struct Chain {
    down: AtomicBool,
}

#[async_trait]
impl ChainClient for Chain {
    async fn chain_id(&self) -> Result<ChainId, ChainError> {
        Ok(CHAIN_ID)
    }

    async fn head_block_number(&self) -> Result<u64, ChainError> {
        if self.down.load(Ordering::SeqCst) {
            return Err(ChainError::Unavailable {
                source: "error sending request for url (https://eth.example/v2/the-providers-key)"
                    .into(),
            });
        }
        Ok(HEAD)
    }

    async fn block_header(&self, _number: u64) -> Result<BlockHeader, ChainError> {
        unreachable!("enrichment reads no headers")
    }
}

struct Case {
    listings: Arc<Listings>,
    pending: Arc<PendingTokens>,
    store: Arc<InMemoryStore>,
    reader: Arc<Reader>,
    chain: Arc<Chain>,
    processor: TokenEnrichmentProcessor,
}

impl Case {
    fn new() -> Self {
        Self::with_concurrency(4)
    }

    fn with_concurrency(concurrency: usize) -> Self {
        Self::over(Arc::default(), Arc::default(), Arc::default(), concurrency)
    }

    fn over(
        listings: Arc<Listings>,
        pending: Arc<PendingTokens>,
        store: Arc<InMemoryStore>,
        concurrency: usize,
    ) -> Self {
        let (reader, chain) = (Arc::new(Reader::default()), Arc::new(Chain::default()));
        let processor = TokenEnrichmentProcessor::new(
            CHAIN_ID,
            EnrichmentSettings {
                retry_delay: RETRY,
                concurrency,
            },
            Box::new(Arc::clone(&listings)),
            Arc::clone(&pending),
            Box::new(Arc::clone(&store)),
            Arc::clone(&reader) as Arc<dyn Erc20MetadataReader>,
            Arc::clone(&chain) as Arc<dyn ChainClient>,
        );
        Self {
            listings,
            pending,
            store,
            reader,
            chain,
            processor,
        }
    }

    /// The same collaborators behind a new processor, as a restart leaves them, minus the buffer.
    fn restarted(&self) -> Self {
        Self::over(
            Arc::clone(&self.listings),
            Arc::default(),
            Arc::clone(&self.store),
            4,
        )
    }

    fn listing(&self, tokens: &[Address]) {
        *self.listings.listed.lock().unwrap() = tokens.to_vec();
    }

    fn dispatch_now(&self, cancel: &CancellationToken) -> Option<Outcome> {
        self.processor
            .on_block_range(100, 200, cancel)
            .now_or_never()
            .map(Result::unwrap)
    }

    async fn dispatch(&self) {
        assert_eq!(
            self.dispatch_now(&CancellationToken::new()),
            Some(Outcome::Ok)
        );
        self.processor.settled().await;
    }

    async fn caught_up_on(&self, tokens: &[Address]) {
        self.listing(tokens);
        self.dispatch().await;
        assert_eq!(self.store.len(), tokens.len());
    }

    fn listing_calls(&self) -> usize {
        self.listings.calls.load(Ordering::SeqCst)
    }

    fn stored(&self, token: Address) -> Option<TokenMetadataRow> {
        self.store.row(CHAIN_ID, token)
    }
}

fn unreached() -> Field<String> {
    Field::Unreached("timeout".to_owned())
}

#[tokio::test(start_paused = true)]
async fn hands_the_loop_its_outcome_before_the_listings_answer() {
    let case = Case::new();
    case.listings.stalled.store(true, Ordering::SeqCst);

    assert_eq!(
        case.dispatch_now(&CancellationToken::new()),
        Some(Outcome::Ok)
    );
}

#[tokio::test(start_paused = true)]
async fn hands_the_loop_its_outcome_before_the_store_answers() {
    let case = Case::new();
    case.caught_up_on(&[USDC]).await;
    case.store.stalled.store(true, Ordering::SeqCst);
    case.pending.add(&[WETH]);

    assert_eq!(
        case.dispatch_now(&CancellationToken::new()),
        Some(Outcome::Ok)
    );
}

#[tokio::test(start_paused = true)]
async fn returns_while_an_erc20_read_is_still_outstanding() {
    let case = Case::new();
    case.listing(&[USDC]);
    case.reader.gated.store(true, Ordering::SeqCst);

    case.dispatch_now(&CancellationToken::new());
    case.reader.entered.notified().await;
    assert_eq!(case.store.len(), 0);

    case.reader.release();
    case.processor.settled().await;
    assert!(case.stored(USDC).is_some());
}

#[tokio::test(start_paused = true)]
async fn starts_nothing_while_a_run_is_in_flight() {
    let case = Case::new();
    case.listing(&[USDC]);
    case.reader.gated.store(true, Ordering::SeqCst);

    case.dispatch_now(&CancellationToken::new());
    case.reader.entered.notified().await;
    case.dispatch_now(&CancellationToken::new());
    case.dispatch_now(&CancellationToken::new());
    case.reader.release();
    case.processor.settled().await;

    assert_eq!(case.listing_calls(), 1);
    assert_eq!(case.reader.reads(), 1);
}

#[tokio::test(start_paused = true)]
async fn backs_off_after_a_run_panicked_then_asks_for_everything() {
    let case = Case::new();
    case.caught_up_on(&[USDC]).await;
    case.pending.add(&[WETH]);
    case.reader.panics.store(true, Ordering::SeqCst);
    case.dispatch().await;
    case.reader.panics.store(false, Ordering::SeqCst);
    case.listing(&[USDC, WETH]);

    case.dispatch().await;
    assert_eq!(case.listing_calls(), 1);

    tokio::time::advance(RETRY).await;
    case.dispatch().await;
    assert_eq!(case.listing_calls(), 2);
    assert!(case.stored(WETH).is_some());
}

#[tokio::test(start_paused = true)]
async fn starts_nothing_once_shutdown_has_begun() {
    let case = Case::new();
    case.listing(&[USDC]);
    let cancelled = CancellationToken::new();
    cancelled.cancel();

    assert_eq!(case.dispatch_now(&cancelled), Some(Outcome::Ok));
    case.processor.settled().await;

    assert_eq!(case.listing_calls(), 0);
}

#[tokio::test]
async fn does_nothing_on_a_reorg_because_a_fork_does_not_unmint_an_erc20() {
    let case = Case::new();

    let outcome = case
        .processor
        .on_reorg(100, 200, &CancellationToken::new())
        .await;

    assert_eq!(outcome.unwrap(), Outcome::Ok);
    assert_eq!(case.listing_calls(), 0);
}

#[tokio::test(start_paused = true)]
async fn asks_for_the_whole_listing_set_the_first_time() {
    let case = Case::new();
    case.listing(&[USDC]);

    case.dispatch().await;

    assert_eq!(case.listing_calls(), 1);
    assert!(case.stored(USDC).is_some());
}

#[tokio::test(start_paused = true)]
async fn asks_no_one_once_caught_up_with_nothing_listed() {
    let case = Case::new();
    case.caught_up_on(&[USDC]).await;
    let labels_read = case.store.label_reads.load(Ordering::SeqCst);

    case.dispatch().await;
    case.dispatch().await;

    assert_eq!(case.listing_calls(), 1);
    assert_eq!(case.store.label_reads.load(Ordering::SeqCst), labels_read);
    assert_eq!(case.reader.reads(), 1);
}

#[tokio::test(start_paused = true)]
async fn enriches_a_token_the_moment_ingestion_pushes_it() {
    let case = Case::new();
    case.caught_up_on(&[USDC]).await;

    case.pending.add(&[WETH]);
    case.dispatch().await;

    assert!(case.stored(WETH).is_some());
    assert_eq!(case.listing_calls(), 1);
}

#[tokio::test(start_paused = true)]
async fn drains_the_buffer_even_during_a_full_check() {
    let case = Case::new();
    case.listing(&[USDC]);
    case.pending.add(&[USDC]);

    case.dispatch().await;
    assert!(case.pending.is_empty());
    case.dispatch().await;

    assert_eq!(case.listing_calls(), 1);
}

#[tokio::test(start_paused = true)]
async fn still_enriches_a_token_whose_push_was_lost_to_a_restart() {
    let before = Case::new();
    before.pending.add(&[WETH]);

    let after = before.restarted();
    after.listing(&[WETH]);
    after.dispatch().await;

    assert!(after.stored(WETH).is_some());
}

#[tokio::test(start_paused = true)]
async fn goes_back_to_the_whole_set_after_a_failure_not_to_the_buffer() {
    let case = Case::new();
    case.caught_up_on(&[USDC]).await;

    case.pending.add(&[WETH]);
    case.reader.answer(
        WETH,
        TokenMetadata {
            symbol: unreached(),
            ..usdc()
        },
    );
    case.dispatch().await;
    assert_eq!(case.listing_calls(), 1);
    assert!(case.stored(WETH).is_none());

    tokio::time::advance(RETRY).await;
    case.listing(&[USDC, WETH]);
    case.reader.answers.lock().unwrap().clear();
    case.dispatch().await;

    assert_eq!(case.listing_calls(), 2);
    assert!(case.stored(WETH).is_some());
}

#[tokio::test(start_paused = true)]
async fn waits_out_the_delay_after_a_gap_is_left_open() {
    let case = Case::new();
    case.listing(&[USDC]);
    case.chain.down.store(true, Ordering::SeqCst);
    case.dispatch().await;

    tokio::time::advance(RETRY - Duration::from_millis(1)).await;
    case.dispatch().await;

    assert_eq!(case.listing_calls(), 1);
}

#[tokio::test(start_paused = true)]
async fn tries_again_once_the_delay_has_passed() {
    let case = Case::new();
    case.listing(&[USDC]);
    case.chain.down.store(true, Ordering::SeqCst);
    case.dispatch().await;

    tokio::time::advance(RETRY).await;
    case.chain.down.store(false, Ordering::SeqCst);
    case.dispatch().await;

    assert!(case.stored(USDC).is_some());
}

#[tokio::test(start_paused = true)]
async fn imposes_no_delay_after_a_run_that_closed_every_gap() {
    let case = Case::new();
    case.caught_up_on(&[USDC]).await;

    case.pending.add(&[WETH]);
    case.dispatch().await;

    assert!(case.stored(WETH).is_some());
}

#[tokio::test(start_paused = true)]
async fn says_why_it_backed_off_without_the_providers_url() {
    test_tracing::interesting();
    let (captured, lines) = test_tracing::Captured::new();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .without_time()
            .with_writer(captured),
    );
    let case = Case::new();
    case.listing(&[USDC]);
    case.chain.down.store(true, Ordering::SeqCst);

    case.dispatch().with_subscriber(subscriber).await;

    let logged = lines.read();
    assert!(logged.contains("enrichment incomplete"), "{logged}");
    assert!(logged.contains("the chain could not be read"), "{logged}");
    assert!(!logged.contains("the-providers-key"), "{logged}");
}

#[tokio::test(start_paused = true)]
async fn stores_every_listed_token_the_store_lacks_as_read_at_the_head() {
    let case = Case::new();
    case.listing(&[USDC, WETH]);
    case.reader.answer(
        WETH,
        TokenMetadata {
            symbol: Field::Value("WETH".to_owned()),
            name: Field::Value("Wrapped Ether".to_owned()),
            decimals: Field::Value(18),
        },
    );

    case.dispatch().await;

    assert_eq!(
        case.stored(USDC),
        Some(TokenMetadataRow {
            chain_id: CHAIN_ID,
            token: USDC,
            symbol: Some("USDC".to_owned()),
            name: Some("USD Coin".to_owned()),
            token_decimals: Some(6),
            fetched_at_block: HEAD,
        })
    );
    assert_eq!(
        case.stored(WETH).and_then(|row| row.symbol),
        Some("WETH".to_owned())
    );
}

#[tokio::test(start_paused = true)]
async fn reads_at_the_chain_head_not_at_the_range_it_was_handed() {
    let case = Case::new();
    case.listing(&[USDC]);

    case.dispatch().await;

    assert_eq!(*case.reader.seen.lock().unwrap(), [(USDC, HEAD)]);
}

#[tokio::test(start_paused = true)]
async fn skips_a_token_it_already_has() {
    let case = Case::new();
    case.caught_up_on(&[USDC]).await;

    case.pending.add(&[USDC]);
    case.dispatch().await;

    assert_eq!(case.reader.reads(), 1);
}

#[tokio::test(start_paused = true)]
async fn asks_the_store_nothing_when_the_hub_has_listed_nothing() {
    let case = Case::new();

    case.dispatch().await;

    assert_eq!(case.store.label_reads.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn stores_a_token_that_answered_with_nothing() {
    let case = Case::new();
    case.listing(&[USDC]);
    case.reader.answer(
        USDC,
        TokenMetadata {
            symbol: Field::Absent(Absence::NoData),
            name: Field::Absent(Absence::Reverted),
            decimals: Field::Absent(Absence::OutOfRange),
        },
    );

    case.dispatch().await;

    let row = case
        .stored(USDC)
        .expect("the question is recorded as asked");
    assert_eq!(
        (row.symbol, row.name, row.token_decimals),
        (None, None, None)
    );
}

#[tokio::test(start_paused = true)]
async fn leaves_the_gap_open_when_any_call_never_reached_the_token() {
    let case = Case::new();
    case.listing(&[USDC]);
    case.reader.answer(
        USDC,
        TokenMetadata {
            name: unreached(),
            ..usdc()
        },
    );

    case.dispatch().await;

    assert!(case.stored(USDC).is_none());
}

#[tokio::test(start_paused = true)]
async fn keeps_what_it_read_when_shutdown_lands_mid_run() {
    let case = Case::new();
    case.listing(&[USDC]);
    case.reader.gated.store(true, Ordering::SeqCst);
    let shutdown = CancellationToken::new();

    case.dispatch_now(&shutdown);
    case.reader.entered.notified().await;
    shutdown.cancel();
    case.reader.release();
    case.processor.settled().await;

    assert!(case.stored(USDC).is_some());
}

#[tokio::test(start_paused = true)]
async fn reads_at_most_the_configured_number_of_tokens_at_once() {
    let case = Case::with_concurrency(4);
    let tokens: Vec<Address> = (1..=9).map(Address::repeat_byte).collect();
    case.listing(&tokens);

    case.dispatch().await;

    assert_eq!(case.reader.peak.load(Ordering::SeqCst), 4);
    assert_eq!(case.reader.reads(), 9);
}
