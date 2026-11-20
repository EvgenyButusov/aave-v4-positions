//! An event's arguments as JSON, byte for byte as the TypeScript indexer wrote them from viem 2.55.10's
//! `decodeEventLog`, since both write the same ledgers and their views read these fields.

use alloy_dyn_abi::{DecodedEvent, DynSolValue};
use alloy_json_abi::{Event, Param};
use alloy_primitives::hex;

/// viem decodes an integer of up to 48 bits to a JS number and anything wider to a `bigint`, which
/// the TypeScript stored as a decimal string; a reader parsing either form depends on the split.
const WIDEST_NUMBER: usize = 48;

/// Indexed arguments first, then the rest, each in ABI order — the order viem assigns them in, and
/// so the order of the keys. An event with an unnamed argument is an array in ABI order instead.
pub(super) fn render(event: &Event, decoded: &DecodedEvent) -> String {
    let mut json = String::new();
    let indexed = event.inputs.iter().filter(|input| input.indexed);
    let unindexed = event.inputs.iter().filter(|input| !input.indexed);
    let named = event.inputs.iter().all(|input| !input.name.is_empty());

    if named {
        let arguments = indexed
            .zip(&decoded.indexed)
            .chain(unindexed.zip(&decoded.body))
            .map(|(input, value)| (input.name.as_str(), input.components.as_slice(), value));
        object(&mut json, arguments);
    } else {
        let (mut topics, mut data) = (decoded.indexed.iter(), decoded.body.iter());
        let arguments = event.inputs.iter().filter_map(|input| {
            let value = if input.indexed {
                topics.next()
            } else {
                data.next()
            };
            value.map(|value| (input.components.as_slice(), value))
        });
        array(&mut json, arguments);
    }

    json
}

fn value(json: &mut String, components: &[Param], value: &DynSolValue) {
    match value {
        DynSolValue::Bool(flag) => json.push_str(if *flag { "true" } else { "false" }),
        DynSolValue::Uint(number, bits) if *bits <= WIDEST_NUMBER => {
            json.push_str(&number.to_string())
        }
        DynSolValue::Int(number, bits) if *bits <= WIDEST_NUMBER => {
            json.push_str(&number.to_string())
        }
        DynSolValue::Uint(number, _) => string(json, &number.to_string()),
        DynSolValue::Int(number, _) => string(json, &number.to_string()),
        DynSolValue::Address(address) => string(json, &address.to_checksum(None)),
        DynSolValue::FixedBytes(word, size) => string(json, &hex::encode_prefixed(&word[..*size])),
        DynSolValue::Function(function) => string(json, &hex::encode_prefixed(function)),
        DynSolValue::Bytes(bytes) => string(json, &hex::encode_prefixed(bytes)),
        DynSolValue::String(text) => string(json, text),
        DynSolValue::Array(items) | DynSolValue::FixedArray(items) => {
            array(json, items.iter().map(|item| (components, item)));
        }
        DynSolValue::Tuple(fields) if components.iter().all(|field| !field.name.is_empty()) => {
            object(
                json,
                components.iter().zip(fields).map(|(field, value)| {
                    (field.name.as_str(), field.components.as_slice(), value)
                }),
            );
        }
        DynSolValue::Tuple(fields) => array(
            json,
            components
                .iter()
                .zip(fields)
                .map(|(field, value)| (field.components.as_slice(), value)),
        ),
    }
}

fn object<'a>(
    json: &mut String,
    entries: impl Iterator<Item = (&'a str, &'a [Param], &'a DynSolValue)>,
) {
    json.push('{');
    for (index, (key, components, entry)) in entries.enumerate() {
        if index > 0 {
            json.push(',');
        }
        string(json, key);
        json.push(':');
        value(json, components, entry);
    }
    json.push('}');
}

fn array<'a>(json: &mut String, items: impl Iterator<Item = (&'a [Param], &'a DynSolValue)>) {
    json.push('[');
    for (index, (components, item)) in items.enumerate() {
        if index > 0 {
            json.push(',');
        }
        value(json, components, item);
    }
    json.push(']');
}

/// Escaped as `JSON.stringify` escapes: the two-character forms where JSON has one, `\u00XX`
/// otherwise below a space, and everything else as itself.
fn string(json: &mut String, text: &str) {
    json.push('"');
    for character in text.chars() {
        match character {
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            '\u{08}' => json.push_str("\\b"),
            '\u{0C}' => json.push_str("\\f"),
            '\n' => json.push_str("\\n"),
            '\r' => json.push_str("\\r"),
            '\t' => json.push_str("\\t"),
            control if control < ' ' => json.push_str(&format!("\\u{:04x}", u32::from(control))),
            other => json.push(other),
        }
    }
    json.push('"');
}
