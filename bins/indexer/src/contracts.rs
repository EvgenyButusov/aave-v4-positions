//! The Aave contracts followed, each with the processor that indexes it.

#[cfg(test)]
mod tests;

use std::sync::Arc;

use aave::events::{Contract, EventLedger};
use indexing::{BlockProcessor, LogReader};

use crate::config::Config;
use crate::processor::AaveEventProcessor;

pub(crate) fn processors(
    config: &Config,
    logs: &Arc<dyn LogReader>,
    ledger: impl Fn(Contract) -> Box<dyn EventLedger>,
) -> Vec<Box<dyn BlockProcessor>> {
    [(Contract::Spoke, config.spoke), (Contract::Hub, config.hub)]
        .into_iter()
        .map(|(contract, address)| -> Box<dyn BlockProcessor> {
            Box::new(AaveEventProcessor::new(
                config.chain.chain_id,
                contract,
                address,
                Arc::clone(logs),
                ledger(contract),
            ))
        })
        .collect()
}
