//! The Spoke and Hub processors, the Hub's handing each listing it writes to enrichment.

#[cfg(test)]
mod tests;

use std::sync::Arc;

use aave::events::{AaveEventProcessor, Contract, EventLedger};
use alloy_primitives::Address;
use indexing::{BlockProcessor, LogReader};
use token_metadata::PendingTokens;
use units::ChainId;

pub(crate) fn ingestion(
    chain_id: ChainId,
    contracts: [(Contract, Address); 2],
    logs: &Arc<dyn LogReader>,
    ledger: impl Fn(Contract) -> Box<dyn EventLedger>,
    listed: &Arc<PendingTokens>,
) -> Vec<Box<dyn BlockProcessor>> {
    contracts
        .map(|(contract, address)| -> Box<dyn BlockProcessor> {
            let processor = AaveEventProcessor::new(
                chain_id,
                contract,
                address,
                Arc::clone(logs),
                ledger(contract),
            );
            match contract {
                Contract::Spoke => Box::new(processor),
                Contract::Hub => {
                    let listed = Arc::clone(listed);
                    Box::new(processor.on_listed(move |tokens| listed.add(tokens)))
                }
            }
        })
        .into()
}
