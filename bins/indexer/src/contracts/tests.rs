use std::sync::{Arc, Mutex};

use aave::events::{Contract, DecodedEvent, Error as LedgerError, EventLedger};
use async_trait::async_trait;
use indexing::{ChainError, LogFilter, LogReader, Outcome, RawLog};
use tokio_util::sync::CancellationToken;
use units::ChainId;

use super::processors;
use crate::config::Config;

struct NoLogs;

#[async_trait]
impl LogReader for NoLogs {
    async fn logs(&self, _filter: &LogFilter) -> Result<Vec<RawLog>, ChainError> {
        Ok(Vec::new())
    }
}

struct RecordingLedger {
    contract: Contract,
    reverted: Arc<Mutex<Vec<Contract>>>,
}

#[async_trait]
impl EventLedger for RecordingLedger {
    async fn revert(&self, _chain_id: ChainId, _from: u64, _to: u64) -> Result<(), LedgerError> {
        self.reverted.lock().unwrap().push(self.contract);
        Ok(())
    }

    async fn append(&self, _events: &[DecodedEvent]) -> Result<(), LedgerError> {
        Ok(())
    }
}

#[tokio::test]
async fn follows_the_spoke_then_the_hub_at_the_configured_addresses_each_into_its_own_ledger() {
    let env = [
        ("OTEL_SERVICE_NAME", "indexer-rust"),
        ("CHAIN_ID", "1"),
        ("RPC_URLS", "https://eth.example/v2/the-providers-key"),
        (
            "MAIN_SPOKE_ADDRESS",
            "0x1111111111111111111111111111111111111111",
        ),
        (
            "CORE_HUB_ADDRESS",
            "0x2222222222222222222222222222222222222222",
        ),
    ]
    .into_iter()
    .collect();
    let logs: Arc<dyn LogReader> = Arc::new(NoLogs);
    let reverted = Arc::default();

    let followed = processors(&Config::parse(&env).unwrap(), &logs, |contract| {
        Box::new(RecordingLedger {
            contract,
            reverted: Arc::clone(&reverted),
        })
    });

    let names: Vec<_> = followed.iter().map(|processor| processor.name()).collect();
    assert_eq!(names, ["aave-spoke(0x11111111)", "aave-hub(0x22222222)"]);
    for processor in &followed {
        let outcome = processor.on_reorg(1, 1, &CancellationToken::new()).await;
        assert_eq!(outcome.unwrap(), Outcome::Ok);
    }
    assert_eq!(*reverted.lock().unwrap(), [Contract::Spoke, Contract::Hub]);
}
