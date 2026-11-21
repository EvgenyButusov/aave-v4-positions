use alloy_dyn_abi::{DynSolValue, EventExt as _};
use alloy_json_abi::Event;
use alloy_primitives::{Address, B256, Bytes, I256};
use serde_json::Value;

use super::body;
use crate::events::fixtures::{address, decoder, golden};
use crate::events::{Contract, Reason};

fn refusal(contract: Contract, log: indexing::RawLog) -> Reason {
    decoder(contract).decode(&[log]).unwrap_err().reason
}

fn captured(contract: Contract, event: &str) -> indexing::RawLog {
    golden(contract)
        .into_iter()
        .find(|(_, row)| row["event_name"] == event)
        .unwrap()
        .0
}

#[test]
fn decodes_every_followed_event_of_both_contracts() {
    for contract in [Contract::Spoke, Contract::Hub] {
        let logs: Vec<_> = golden(contract).into_iter().map(|(log, _)| log).collect();

        let mut decoded: Vec<_> = decoder(contract)
            .decode(&logs)
            .unwrap()
            .into_iter()
            .map(|event| event.event_name)
            .collect();
        decoded.sort_unstable();
        decoded.dedup();
        let mut followed = contract.events().to_vec();
        followed.sort_unstable();

        assert_eq!(decoded, followed, "{contract}");
    }
}

#[test]
fn renders_each_captured_log_as_the_body_the_typescript_wrote_for_it() {
    for contract in [Contract::Spoke, Contract::Hub] {
        for (log, written) in golden(contract) {
            let event = decoder(contract).decode(&[log]).unwrap().remove(0);

            assert_eq!(
                event.body, written["body"],
                "{contract} {}",
                event.event_name
            );
        }
    }
}

#[test]
fn keeps_the_indexed_parameters_as_the_words_they_arrived_as() {
    let log = captured(Contract::Spoke, "Supply");

    let event = decoder(Contract::Spoke)
        .decode(std::slice::from_ref(&log))
        .unwrap()
        .remove(0);

    assert_eq!(
        event.topics,
        [
            Some(log.topics[1]),
            Some(log.topics[2]),
            Some(log.topics[3])
        ]
    );
    assert_eq!(event.data, log.data);
}

#[test]
fn leaves_the_topic_an_event_does_not_index_empty() {
    let log = captured(Contract::Spoke, "ReportDeficit");

    let event = decoder(Contract::Spoke).decode(&[log]).unwrap().remove(0);

    assert_eq!(event.topics[2], None);
}

#[test]
fn refuses_a_log_another_contract_emitted() {
    let mut log = captured(Contract::Spoke, "Supply");
    log.address = Address::with_last_byte(1);

    assert!(matches!(
        refusal(Contract::Spoke, log),
        Reason::EmittedElsewhere { .. }
    ));
}

#[test]
fn refuses_the_hubs_report_deficit_rather_than_decoding_it_as_the_spokes() {
    let mut log = captured(Contract::Hub, "ReportDeficit");
    log.address = address(Contract::Spoke);

    assert!(matches!(
        refusal(Contract::Spoke, log),
        Reason::UnknownSignature { .. }
    ));
}

#[test]
fn refuses_an_event_of_the_contract_that_was_not_asked_for() {
    let mut log = captured(Contract::Hub, "Add");
    let transfer = Contract::Hub.abi().event("TransferShares").unwrap()[0].selector();
    log.topics[0] = transfer;

    assert!(matches!(
        refusal(Contract::Hub, log),
        Reason::NotFollowed {
            event: "TransferShares",
            ..
        }
    ));
}

#[test]
fn refuses_an_anonymous_log() {
    let mut log = captured(Contract::Spoke, "Supply");
    log.topics.clear();

    assert!(matches!(refusal(Contract::Spoke, log), Reason::NoSignature));
}

#[test]
fn refuses_a_log_whose_data_is_short_of_its_abi() {
    let mut log = captured(Contract::Spoke, "Supply");
    log.data = Bytes::from_static(&[0; 32]);

    assert!(matches!(
        refusal(Contract::Spoke, log),
        Reason::DoesNotMatchAbi(_)
    ));
}

#[test]
fn names_the_log_it_refused() {
    let mut log = captured(Contract::Spoke, "Supply");
    log.address = Address::with_last_byte(1);
    let (block, index) = (log.block_number, log.log_index);

    let refused = decoder(Contract::Spoke).decode(&[log]).unwrap_err();

    assert_eq!((refused.block_number, refused.log_index), (block, index));
    assert!(
        refused
            .to_string()
            .starts_with(&format!("cannot decode log {block}:{index}: "))
    );
}

/// An event no followed ABI has: an unnamed argument, a narrow signed integer, and a string that
/// needs escaping.
fn synthetic() -> Event {
    serde_json::from_value(serde_json::json!({
        "type": "event",
        "name": "Synthetic",
        "anonymous": false,
        "inputs": [
            { "name": "", "type": "int16", "indexed": true },
            { "name": "", "type": "string", "indexed": false },
            { "name": "", "type": "bytes4", "indexed": false }
        ]
    }))
    .unwrap()
}

#[test]
fn renders_an_event_with_an_unnamed_argument_as_an_array_in_abi_order() {
    let event = synthetic();
    let minus_two = B256::from(I256::try_from(-2).unwrap().to_be_bytes::<32>());
    let data = DynSolValue::Tuple(vec![
        DynSolValue::String("a \"b\"\n\u{1}".to_owned()),
        DynSolValue::FixedBytes(B256::ZERO, 4),
    ])
    .abi_encode_params();
    let decoded = event
        .decode_log_parts([event.selector(), minus_two], &data)
        .unwrap();

    let json = body::render(&event, &decoded);

    assert_eq!(json, r#"[-2,"a \"b\"\n\u0001","0x00000000"]"#);
    assert!(serde_json::from_str::<Value>(&json).is_ok());
}

#[test]
fn renders_the_types_no_followed_event_carries_as_viem_did() {
    let shapes: Value =
        serde_json::from_str(include_str!("../../../fixtures/shapes.json")).unwrap();
    let event: Event = serde_json::from_value(shapes["event"].clone()).unwrap();
    let topics: Vec<B256> = serde_json::from_value(shapes["topics"].clone()).unwrap();
    let data: Bytes = shapes["data"].as_str().unwrap().parse().unwrap();

    let decoded = event.decode_log_parts(topics, &data).unwrap();

    assert_eq!(
        body::render(&event, &decoded),
        shapes["body"].as_str().unwrap()
    );
}
