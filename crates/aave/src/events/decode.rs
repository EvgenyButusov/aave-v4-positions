mod body;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use alloy_dyn_abi::EventExt as _;
use alloy_json_abi::Event;
use alloy_primitives::{Address, B256, Bytes};
use indexing::RawLog;
use units::ChainId;

use crate::events::Contract;

/// One log as its ledger keeps it: the envelope, the indexed parameters as the words they arrived
/// as, every argument as JSON, and the raw data, so a decoder bug is repaired by re-decoding what is
/// stored rather than re-fetching the chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedEvent {
    pub chain_id: ChainId,
    pub address: Address,
    pub block_number: u64,
    pub block_hash: B256,
    /// Unix seconds.
    pub block_timestamp: u32,
    pub transaction_hash: B256,
    pub transaction_index: u32,
    pub log_index: u32,
    pub event_name: &'static str,
    /// `topic1` to `topic3`. Only `topic1` means the same field on every event of a contract.
    pub topics: [Option<B256>; 3],
    pub body: String,
    pub data: Bytes,
}

/// Turns one contract's logs into ledger rows, refusing any it was not asked for.
pub struct Decoder {
    chain_id: ChainId,
    contract: Contract,
    address: Address,
    by_signature: HashMap<B256, &'static Event>,
}

#[derive(Debug, thiserror::Error)]
#[error("cannot decode log {block_number}:{log_index}: {reason}")]
pub struct Undecodable {
    pub block_number: u64,
    pub log_index: u64,
    pub reason: Reason,
}

#[derive(Debug, thiserror::Error)]
pub enum Reason {
    #[error("emitted by {emitter:#x}, not the configured {contract} {expected:#x}")]
    EmittedElsewhere {
        emitter: Address,
        contract: Contract,
        expected: Address,
    },

    #[error("anonymous event, no topic0")]
    NoSignature,

    #[error("topic0 {topic0:#x} is no {contract} event")]
    UnknownSignature { topic0: B256, contract: Contract },

    #[error("{event} is not one of the {contract} events that were requested")]
    NotFollowed {
        event: &'static str,
        contract: Contract,
    },

    #[error("the log does not match its event's ABI")]
    DoesNotMatchAbi(#[source] alloy_dyn_abi::Error),

    #[error("its {0} does not fit the ledger's column")]
    OutOfRange(&'static str),
}

impl DecodedEvent {
    /// The ERC-20 a Hub `AddAsset` listed, from its indexed `underlying`. No other event lists one.
    #[must_use]
    pub fn listed_token(&self) -> Option<Address> {
        (self.event_name == "AddAsset")
            .then_some(self.topics[1])
            .flatten()
            .map(Address::from_word)
    }
}

impl Decoder {
    #[must_use]
    pub fn new(chain_id: ChainId, contract: Contract, address: Address) -> Self {
        let by_signature = contract
            .abi()
            .events()
            .map(|event| (event.selector(), event))
            .collect();

        Self {
            chain_id,
            contract,
            address,
            by_signature,
        }
    }

    /// # Errors
    ///
    /// [`Undecodable`] for the first log that is not one of this contract's followed events: the
    /// provider was asked for exactly those, so anything else is a wrong ABI or a wrong filter.
    pub fn decode(&self, logs: &[RawLog]) -> Result<Vec<DecodedEvent>, Undecodable> {
        logs.iter()
            .map(|log| {
                self.decode_one(log).map_err(|reason| Undecodable {
                    block_number: log.block_number,
                    log_index: log.log_index,
                    reason,
                })
            })
            .collect()
    }

    fn decode_one(&self, log: &RawLog) -> Result<DecodedEvent, Reason> {
        if log.address != self.address {
            return Err(Reason::EmittedElsewhere {
                emitter: log.address,
                contract: self.contract,
                expected: self.address,
            });
        }
        let &topic0 = log.topics.first().ok_or(Reason::NoSignature)?;
        let &event = self
            .by_signature
            .get(&topic0)
            .ok_or(Reason::UnknownSignature {
                topic0,
                contract: self.contract,
            })?;
        if !self.contract.follows(&event.name) {
            return Err(Reason::NotFollowed {
                event: &event.name,
                contract: self.contract,
            });
        }

        let decoded = event
            .decode_log_parts(log.topics.iter().copied(), &log.data)
            .map_err(Reason::DoesNotMatchAbi)?;
        let mut indexed = log.topics.iter().skip(1).copied();

        Ok(DecodedEvent {
            chain_id: self.chain_id,
            address: self.address,
            block_number: log.block_number,
            block_hash: log.block_hash,
            block_timestamp: fits(log.block_timestamp, "block_timestamp")?,
            transaction_hash: log.transaction_hash,
            transaction_index: fits(log.transaction_index, "transaction_index")?,
            log_index: fits(log.log_index, "log_index")?,
            event_name: &event.name,
            topics: [indexed.next(), indexed.next(), indexed.next()],
            body: body::render(event, &decoded),
            data: log.data.clone(),
        })
    }
}

fn fits(value: u64, field: &'static str) -> Result<u32, Reason> {
    u32::try_from(value).map_err(|_| Reason::OutOfRange(field))
}
