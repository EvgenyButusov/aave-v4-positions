use std::sync::Arc;

use aave::MAIN_SPOKE_ON_ETHEREUM as FOLLOWED;
use aave::events::fixtures::{address, golden};
use aave::events::{Contract, DecodedEvent, Error, EventLedger};
use alloy_primitives::Address;
use async_trait::async_trait;
use indexing::{ChainError, LogFilter, LogReader, Outcome, RawLog};
use token_metadata::PendingTokens;
use tokio_util::sync::CancellationToken;
use units::ChainId;

use super::ingestion;

/// Each contract's captured logs, to whichever processor asks for that contract's address.
struct Captured;

#[async_trait]
impl LogReader for Captured {
    async fn logs(&self, filter: &LogFilter) -> Result<Vec<RawLog>, ChainError> {
        let contract = [Contract::Spoke, Contract::Hub]
            .into_iter()
            .find(|&contract| filter.addresses == [address(contract)])
            .expect("asked for a followed contract");
        Ok(golden(contract).into_iter().map(|(log, _)| log).collect())
    }
}

struct Discarding;

#[async_trait]
impl EventLedger for Discarding {
    async fn revert(&self, _chain_id: ChainId, _from: u64, _to: u64) -> Result<(), Error> {
        Ok(())
    }

    async fn append(&self, _events: &[DecodedEvent]) -> Result<(), Error> {
        Ok(())
    }
}

#[tokio::test]
async fn hands_what_the_hub_lists_to_enrichment_and_nothing_from_the_spoke() {
    let listed = Arc::new(PendingTokens::default());
    let processors = ingestion(
        FOLLOWED.chain_id,
        [
            (Contract::Spoke, address(Contract::Spoke)),
            (Contract::Hub, address(Contract::Hub)),
        ],
        &(Arc::new(Captured) as Arc<dyn LogReader>),
        |_| Box::new(Discarding),
        &listed,
    );

    for processor in &processors {
        let outcome = processor
            .on_block_range(1_000, 1_999, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(outcome, Outcome::Ok, "{}", processor.name());
    }

    let weth: Address = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
        .parse()
        .unwrap();
    assert_eq!(listed.drain(), [weth]);
}
