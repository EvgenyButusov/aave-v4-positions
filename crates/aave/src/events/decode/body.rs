//! An event's arguments as JSON, byte for byte as the TypeScript indexer wrote them from viem 2.55.10's
//! `decodeEventLog`, since both write the same ledgers and their views read these fields.

use alloy_dyn_abi::{DecodedEvent, DynSolValue};
use alloy_json_abi::{Event, Param};
use alloy_primitives::hex;
use serde::{Serialize, Serializer};

/// viem decodes an integer of up to 48 bits to a JS number and anything wider to a `bigint`, which
/// the TypeScript stored as a decimal string; a reader parsing either form depends on the split.
const WIDEST_NUMBER: usize = 48;

#[expect(
    clippy::expect_used,
    reason = "serde_json refuses only a non-string map key, and every key here is a parameter name"
)]
pub(super) fn render(event: &Event, decoded: &DecodedEvent) -> String {
    serde_json::to_string(&Arguments { event, decoded }).expect("arguments that serialize")
}

// Streamed rather than built as a `serde_json::Value`, whose map sorts its keys.
struct Arguments<'a> {
    event: &'a Event,
    decoded: &'a DecodedEvent,
}

struct Value<'a> {
    components: &'a [Param],
    value: &'a DynSolValue,
}

impl Serialize for Arguments<'_> {
    /// Indexed arguments first, then the rest, each in ABI order — the order viem assigns them in,
    /// and so the order of the keys. An event with an unnamed argument is an array in ABI order.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (inputs, decoded) = (&self.event.inputs, self.decoded);

        if inputs.iter().all(|input| !input.name.is_empty()) {
            let indexed = inputs.iter().filter(|input| input.indexed);
            let unindexed = inputs.iter().filter(|input| !input.indexed);
            serializer.collect_map(
                indexed
                    .zip(&decoded.indexed)
                    .chain(unindexed.zip(&decoded.body))
                    .map(|(input, value)| {
                        (
                            &input.name,
                            Value {
                                components: &input.components,
                                value,
                            },
                        )
                    }),
            )
        } else {
            let (mut topics, mut data) = (decoded.indexed.iter(), decoded.body.iter());
            serializer.collect_seq(inputs.iter().filter_map(|input| {
                let value = if input.indexed {
                    topics.next()
                } else {
                    data.next()
                };
                value.map(|value| Value {
                    components: &input.components,
                    value,
                })
            }))
        }
    }
}

impl Serialize for Value<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let components = self.components;
        match self.value {
            DynSolValue::Bool(flag) => serializer.serialize_bool(*flag),
            DynSolValue::Uint(number, bits) if *bits <= WIDEST_NUMBER => {
                serializer.serialize_u64(number.to())
            }
            DynSolValue::Int(number, bits) if *bits <= WIDEST_NUMBER => {
                serializer.serialize_i64(number.as_i64())
            }
            DynSolValue::Uint(number, _) => serializer.collect_str(number),
            DynSolValue::Int(number, _) => serializer.collect_str(number),
            DynSolValue::Address(address) => serializer.serialize_str(&address.to_checksum(None)),
            DynSolValue::FixedBytes(word, size) => {
                serializer.serialize_str(&hex::encode_prefixed(&word[..*size]))
            }
            DynSolValue::Function(function) => {
                serializer.serialize_str(&hex::encode_prefixed(function))
            }
            DynSolValue::Bytes(bytes) => serializer.serialize_str(&hex::encode_prefixed(bytes)),
            DynSolValue::String(text) => serializer.serialize_str(text),
            DynSolValue::Array(items) | DynSolValue::FixedArray(items) => {
                serializer.collect_seq(items.iter().map(|value| Self { components, value }))
            }
            DynSolValue::Tuple(fields) if components.iter().all(|field| !field.name.is_empty()) => {
                serializer.collect_map(components.iter().zip(fields).map(|(field, value)| {
                    (
                        &field.name,
                        Value {
                            components: &field.components,
                            value,
                        },
                    )
                }))
            }
            DynSolValue::Tuple(fields) => {
                serializer.collect_seq(components.iter().zip(fields).map(|(field, value)| Value {
                    components: &field.components,
                    value,
                }))
            }
        }
    }
}
