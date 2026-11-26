use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use indexing::{BlockProcessor as _, ChainError, LogFilter, LogReader, Outcome, RawLog};
use tokio_util::sync::CancellationToken;
use units::ChainId;

use super::AaveEventProcessor;
use crate::MAIN_SPOKE_ON_ETHEREUM as FOLLOWED;
use crate::events::fixtures::{address, decoder, golden};
use crate::events::{Contract, DecodedEvent, Error as LedgerError, EventLedger};

enum Serves {
    Logs(Vec<RawLog>),
    TooWide,
    Down,
}

struct ScriptedChain {
    serves: Serves,
    asked: Mutex<Vec<LogFilter>>,
}

#[async_trait]
impl LogReader for ScriptedChain {
    async fn logs(&self, filter: &LogFilter) -> Result<Vec<RawLog>, ChainError> {
        self.asked.lock().unwrap().push(filter.clone());
        match &self.serves {
            Serves::Logs(logs) => Ok(logs.clone()),
            Serves::TooWide => Err(ChainError::RangeTooLarge {
                from: filter.from_block,
                to: filter.to_block,
                detail: "query returned more than 10000 results".to_owned(),
            }),
            Serves::Down => Err(ChainError::Unavailable {
                source: "connection refused".into(),
            }),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Write {
    Revert { from: u64, to: u64 },
    Append(Vec<&'static str>),
}

#[derive(Clone, Default)]
struct RecordingLedger {
    writes: Arc<Mutex<Vec<Write>>>,
    refuses: bool,
}

#[async_trait]
impl EventLedger for RecordingLedger {
    async fn revert(&self, chain_id: ChainId, from: u64, to: u64) -> Result<(), LedgerError> {
        assert_eq!(chain_id, FOLLOWED.chain_id);
        if self.refuses {
            return Err(LedgerError::RevertFailed {
                contract: Contract::Spoke,
                from,
                to,
                source: "the server refused".into(),
            });
        }
        self.writes.lock().unwrap().push(Write::Revert { from, to });
        Ok(())
    }

    async fn append(&self, events: &[DecodedEvent]) -> Result<(), LedgerError> {
        let names = events.iter().map(|event| event.event_name).collect();
        self.writes.lock().unwrap().push(Write::Append(names));
        Ok(())
    }
}

struct Case {
    processor: AaveEventProcessor,
    chain: Arc<ScriptedChain>,
    ledger: RecordingLedger,
}

impl Case {
    fn new(contract: Contract, serves: Serves) -> Self {
        Self::over(contract, serves, RecordingLedger::default())
    }

    fn over(contract: Contract, serves: Serves, ledger: RecordingLedger) -> Self {
        let chain = Arc::new(ScriptedChain {
            serves,
            asked: Mutex::default(),
        });
        let processor = AaveEventProcessor::new(
            FOLLOWED.chain_id,
            contract,
            address(contract),
            Arc::clone(&chain) as Arc<dyn LogReader>,
            Box::new(ledger.clone()),
        );
        Self {
            processor,
            chain,
            ledger,
        }
    }

    async fn index(&self, from: u64, to: u64) -> Result<Outcome, indexing::Failure> {
        self.processor
            .on_block_range(from, to, &CancellationToken::new())
            .await
    }

    fn writes(&self) -> Vec<Write> {
        std::mem::take(&mut self.ledger.writes.lock().unwrap())
    }
}

fn captured(contract: Contract) -> Vec<RawLog> {
    golden(contract).into_iter().map(|(log, _)| log).collect()
}

#[tokio::test]
async fn asks_the_chain_for_the_contracts_followed_events_over_the_range() {
    for contract in [Contract::Spoke, Contract::Hub] {
        let case = Case::new(contract, Serves::Logs(Vec::new()));

        assert_eq!(case.index(1_000, 1_999).await.unwrap(), Outcome::Ok);

        assert_eq!(
            *case.chain.asked.lock().unwrap(),
            [LogFilter {
                addresses: vec![address(contract)],
                topic0: contract.topics(),
                from_block: 1_000,
                to_block: 1_999,
            }],
            "{contract}"
        );
    }
}

#[tokio::test]
async fn retracts_the_range_then_appends_what_it_decoded() {
    let logs = captured(Contract::Hub);
    let case = Case::new(Contract::Hub, Serves::Logs(logs.clone()));

    let outcome = case.index(1_000, 1_999).await.unwrap();

    let names: Vec<_> = decoder(Contract::Hub)
        .decode(&logs)
        .unwrap()
        .into_iter()
        .map(|event| event.event_name)
        .collect();
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(
        case.writes(),
        [
            Write::Revert {
                from: 1_000,
                to: 1_999
            },
            Write::Append(names)
        ]
    );
}

#[tokio::test]
async fn retracts_a_range_that_now_holds_no_events() {
    let case = Case::new(Contract::Spoke, Serves::Logs(Vec::new()));

    assert_eq!(case.index(1_000, 1_000).await.unwrap(), Outcome::Ok);

    assert_eq!(
        case.writes(),
        [
            Write::Revert {
                from: 1_000,
                to: 1_000
            },
            Write::Append(Vec::new())
        ]
    );
}

#[tokio::test]
async fn asks_for_a_narrower_range_when_the_provider_refuses_the_width() {
    let case = Case::new(Contract::Spoke, Serves::TooWide);

    let outcome = case.index(1_000, 1_999).await.unwrap();

    assert!(matches!(
        outcome,
        Outcome::Retry {
            narrow_range: true,
            ..
        }
    ));
    assert_eq!(case.writes(), []);
}

#[tokio::test]
async fn hands_an_unreachable_chain_back_for_a_retry_of_the_same_range() {
    let case = Case::new(Contract::Spoke, Serves::Down);

    let failure = case.index(1_000, 1_999).await.unwrap_err();

    assert_eq!(failure.to_string(), "the chain could not be read");
    assert_eq!(case.writes(), []);
}

#[tokio::test]
async fn stops_indexing_on_a_log_it_cannot_decode() {
    let case = Case::new(Contract::Spoke, Serves::Logs(captured(Contract::Hub)));

    let outcome = case.index(1_000, 1_999).await.unwrap();

    let Outcome::Failed { reason } = outcome else {
        panic!("{outcome:?}");
    };
    assert!(reason.starts_with("cannot decode log"), "{reason}");
    assert_eq!(case.writes(), []);
}

#[tokio::test]
async fn writes_nothing_once_shutdown_has_begun() {
    let case = Case::new(Contract::Spoke, Serves::Logs(captured(Contract::Spoke)));
    let cancel = CancellationToken::new();
    cancel.cancel();

    let outcome = case
        .processor
        .on_block_range(1_000, 1_999, &cancel)
        .await
        .unwrap();

    assert!(matches!(
        outcome,
        Outcome::Retry {
            narrow_range: false,
            ..
        }
    ));
    assert_eq!(case.writes(), []);
}

#[tokio::test]
async fn hands_a_refused_write_back_for_a_retry() {
    let ledger = RecordingLedger {
        refuses: true,
        ..RecordingLedger::default()
    };
    let case = Case::over(Contract::Spoke, Serves::Logs(Vec::new()), ledger);

    let failure = case.index(1_000, 1_999).await.unwrap_err();

    assert_eq!(
        failure.to_string(),
        "the spoke ledger could not retract blocks 1000..=1999"
    );
    assert_eq!(case.writes(), []);
}

#[tokio::test]
async fn only_retracts_a_reorged_range() {
    let case = Case::new(Contract::Spoke, Serves::Logs(captured(Contract::Spoke)));

    let outcome = case
        .processor
        .on_reorg(1_000, 1_999, &CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(
        case.writes(),
        [Write::Revert {
            from: 1_000,
            to: 1_999
        }]
    );
    assert!(case.chain.asked.lock().unwrap().is_empty());
}

#[test]
fn is_named_for_the_contract_and_its_address() {
    let case = Case::new(Contract::Spoke, Serves::Down);

    assert_eq!(case.processor.name(), "aave-spoke(0x94e7a5dc)");
}
