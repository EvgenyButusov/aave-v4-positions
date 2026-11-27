use alloy_primitives::{Address, B256, U256, address, hex as hexes};
use alloy_sol_types::{SolType as _, sol_data};
use indexing::{Absence, Erc20MetadataReader as _, Field, TokenMetadata};
use serde_json::{Value, json};

use crate::harness::{harness, over_one};
use crate::scripted::{Reply, ScriptedNetwork, UNREACHABLE};

const TOKEN: Address = address!("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");
const AT_BLOCK: u64 = 25_652_782;

#[derive(Clone)]
enum Answer {
    Data(Vec<u8>),
    Revert(&'static str),
}

fn string(value: &str) -> Answer {
    Answer::Data(sol_data::String::abi_encode(value))
}

fn bytes32(value: &[u8]) -> Answer {
    Answer::Data(B256::right_padding_from(value).to_vec())
}

fn word(value: U256) -> Answer {
    Answer::Data(value.to_be_bytes_vec())
}

/// Answers `eth_call` by selector; a method left out returns `0x`, as a missing one does.
fn token(
    symbol: Option<Answer>,
    name: Option<Answer>,
    decimals: Option<Answer>,
) -> impl Fn(&str, &Value) -> Reply + Send + Sync + 'static {
    move |_, params| {
        let answer = match params[0]["data"].as_str().unwrap_or_default() {
            "0x95d89b41" => symbol.clone(),
            "0x06fdde03" => name.clone(),
            "0x313ce567" => decimals.clone(),
            other => panic!("not an ERC-20 metadata selector: {other}"),
        };

        match answer {
            Some(Answer::Data(data)) => Reply::Result(json!(hexes::encode_prefixed(data))),
            Some(Answer::Revert(message)) => Reply::Error { code: 3, message },
            None => Reply::Result(json!("0x")),
        }
    }
}

async fn read(
    symbol: Option<Answer>,
    name: Option<Answer>,
    decimals: Option<Answer>,
) -> TokenMetadata {
    let (_, harness) = over_one(token(symbol, name, decimals)).await;

    harness.rpc.read(TOKEN, AT_BLOCK).await
}

async fn symbol_of(answer: Answer) -> Field<String> {
    read(Some(answer), None, None).await.symbol
}

#[tokio::test]
async fn reads_an_ordinary_token() {
    let metadata = read(
        Some(string("USDC")),
        Some(string("USD Coin")),
        Some(word(U256::from(6))),
    )
    .await;

    assert_eq!(
        metadata,
        TokenMetadata {
            symbol: Field::Value("USDC".to_owned()),
            name: Field::Value("USD Coin".to_owned()),
            decimals: Field::Value(6),
        }
    );
    assert!(metadata.answered());
}

#[tokio::test]
async fn pins_every_call_to_the_block_it_was_given() {
    let (network, harness) = over_one(token(None, None, None)).await;

    harness.rpc.read(TOKEN, AT_BLOCK).await;

    let calls = network.calls();
    assert_eq!(calls.len(), 3);
    for call in calls {
        assert_eq!(call.method, "eth_call");
        assert_eq!(call.params[0]["to"], json!(format!("{TOKEN:#x}")));
        assert_eq!(call.params[1], json!(format!("{AT_BLOCK:#x}")));
    }
}

#[tokio::test]
async fn falls_back_to_bytes32_and_trims_the_padding() {
    assert_eq!(
        symbol_of(bytes32(b"MKR")).await,
        Field::Value("MKR".to_owned())
    );
}

#[tokio::test]
async fn falls_back_whichever_way_the_string_decode_fails() {
    let left_padded = B256::left_padding_from(b"MKR").to_vec();

    assert_eq!(
        symbol_of(Answer::Data(left_padded)).await,
        Field::Value("MKR".to_owned())
    );
}

#[tokio::test]
async fn reports_a_revert_per_field_rather_than_failing_the_read() {
    let metadata = read(
        Some(Answer::Revert("execution reverted: no symbol here")),
        Some(string("Mute Token")),
        Some(word(U256::from(18))),
    )
    .await;

    assert_eq!(metadata.symbol, Field::Absent(Absence::Reverted));
    assert_eq!(metadata.name, Field::Value("Mute Token".to_owned()));
    assert!(metadata.answered());
}

#[tokio::test]
async fn classifies_a_bare_revert_the_same_as_one_carrying_a_reason() {
    assert_eq!(
        symbol_of(Answer::Revert("execution reverted")).await,
        Field::Absent(Absence::Reverted)
    );
}

#[tokio::test]
async fn reports_an_empty_return_which_is_also_what_an_account_without_code_gives() {
    let metadata = read(None, None, None).await;

    assert_eq!(metadata.symbol, Field::Absent(Absence::NoData));
    assert_eq!(metadata.decimals, Field::Absent(Absence::NoData));
    assert!(metadata.answered());
}

#[tokio::test]
async fn reports_a_return_too_short_for_either_shape_as_undecodable() {
    assert_eq!(
        symbol_of(Answer::Data(vec![0x41; 16])).await,
        Field::Absent(Absence::Undecodable)
    );
}

#[tokio::test]
async fn strips_control_characters_from_both_paths() {
    let metadata = read(Some(bytes32(b"MK\x07R")), Some(string("USD\nCoin")), None).await;

    assert_eq!(metadata.symbol, Field::Value("MKR".to_owned()));
    assert_eq!(metadata.name, Field::Value("USDCoin".to_owned()));
}

#[tokio::test]
async fn truncates_a_label_that_is_far_too_long() {
    let name = read(None, Some(string(&"A".repeat(5_000))), None)
        .await
        .name;

    assert_eq!(name, Field::Value("A".repeat(128)));
}

#[tokio::test]
async fn never_ends_a_truncated_label_on_half_a_surrogate_pair() {
    let cut = format!("{}😀😀", "A".repeat(127));

    let name = read(None, Some(string(&cut)), None).await.name;

    assert_eq!(name, Field::Value("A".repeat(127)));
}

#[tokio::test]
async fn reports_a_label_of_only_whitespace_as_blank() {
    assert_eq!(
        symbol_of(string(" \u{feff}\u{a0}\t ")).await,
        Field::Absent(Absence::Blank)
    );
}

#[tokio::test]
async fn keeps_bytes_that_are_not_utf8_rather_than_inventing_a_label() {
    let symbol = symbol_of(Answer::Data(vec![0xff; 32])).await;

    assert_eq!(symbol, Field::Value("\u{fffd}".repeat(32)));
}

#[tokio::test]
async fn refuses_a_decimals_that_cannot_be_a_uint8() {
    let odd = read(None, None, Some(word(U256::from(999)))).await;
    let dirty = read(
        None,
        None,
        Some(word((U256::from(1) << 248) | U256::from(6))),
    )
    .await;

    assert_eq!(odd.decimals, Field::Absent(Absence::OutOfRange));
    assert_eq!(dirty.decimals, Field::Absent(Absence::OutOfRange));
}

#[tokio::test]
async fn leaves_every_field_unreached_when_no_provider_answers() {
    let metadata = harness(&[UNREACHABLE]).rpc.read(TOKEN, AT_BLOCK).await;

    assert!(!metadata.answered());
    for field in [&metadata.symbol, &metadata.name] {
        assert!(matches!(field, Field::Unreached(_)), "{field:?}");
    }
}

#[tokio::test]
async fn names_an_unreached_field_without_the_providers_url() {
    let metadata = harness(&[UNREACHABLE]).rpc.read(TOKEN, AT_BLOCK).await;

    let Field::Unreached(reason) = metadata.symbol else {
        panic!("expected the symbol unreached: {:?}", metadata.symbol);
    };
    assert!(!reason.contains("127.0.0.1"), "{reason}");
}

#[tokio::test]
async fn a_node_error_that_is_not_a_revert_leaves_the_gap_open() {
    let (_, harness) = over_one(|_, _| Reply::Error {
        code: -32000,
        message: "header not found",
    })
    .await;

    let metadata = harness.rpc.read(TOKEN, AT_BLOCK).await;

    assert_eq!(
        metadata.symbol,
        Field::Unreached("JSON-RPC error -32000".to_owned())
    );
    assert!(!metadata.answered());
}

#[tokio::test]
async fn takes_a_revert_as_the_answer_and_asks_no_other_provider() {
    let network = ScriptedNetwork::new();
    let primary = network
        .node(
            "primary",
            token(Some(Answer::Revert("execution reverted")), None, None),
        )
        .await;
    let fallback = network
        .node("fallback", token(Some(string("WRONG")), None, None))
        .await;

    let metadata = harness(&[&primary.url, &fallback.url])
        .rpc
        .read(TOKEN, AT_BLOCK)
        .await;

    assert_eq!(metadata.symbol, Field::Absent(Absence::Reverted));
    assert!(
        network.asked().iter().all(|(node, _)| node == "primary"),
        "{:?}",
        network.asked()
    );
}
