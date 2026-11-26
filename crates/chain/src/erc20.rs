//! ERC-20 metadata over `eth_call`, each of the four hazards answered as a field rather than an
//! error.

#[cfg(test)]
mod tests;

use alloy_primitives::{Address, Bytes, U64};
use alloy_sol_types::{SolCall as _, SolType as _, sol, sol_data};
use alloy_transport::{RpcError, TransportError};
use async_trait::async_trait;
use futures_util::future::join3;
use indexing::{Absence, Erc20MetadataReader, Field, TokenMetadata};
use serde::Serialize;

use crate::json_rpc::JsonRpc;
use crate::providers::{failure, is_revert};

sol! {
    function symbol() external view returns (string);
    function name() external view returns (string);
    function decimals() external view returns (uint8);
}

/// Far above every real label and far below a payload. Counted in UTF-16 units, as the labels
/// already stored were.
const MAX_LABEL_UNITS: usize = 128;

/// No answer from the token: the contract's refusal, or no contact with it at all.
enum Unanswered {
    Reverted,
    Unreached(String),
}

#[derive(Debug, Clone, Serialize)]
struct Call {
    to: Address,
    data: Bytes,
}

#[async_trait]
impl Erc20MetadataReader for JsonRpc {
    async fn read(&self, token: Address, at_block: u64) -> TokenMetadata {
        let (symbol, name, decimals) = join3(
            self.returned(token, &symbolCall::SELECTOR, at_block),
            self.returned(token, &nameCall::SELECTOR, at_block),
            self.returned(token, &decimalsCall::SELECTOR, at_block),
        )
        .await;

        TokenMetadata {
            symbol: symbol.map_or_else(Unanswered::field, |data| label(&data)),
            name: name.map_or_else(Unanswered::field, |data| label(&data)),
            decimals: decimals.map_or_else(Unanswered::field, |data| width(&data)),
        }
    }
}

impl JsonRpc {
    async fn returned(
        &self,
        token: Address,
        selector: &[u8; 4],
        at_block: u64,
    ) -> Result<Bytes, Unanswered> {
        let call = Call {
            to: token,
            data: Bytes::copy_from_slice(selector),
        };

        self.client
            .request("eth_call", (call, U64::from(at_block)))
            .await
            .map_err(|error| Unanswered::of(&error))
    }
}

impl Unanswered {
    /// Named by kind and never by the error's text, which for a transport failure carries the
    /// provider's URL and so its key.
    fn of(error: &TransportError) -> Self {
        match error {
            RpcError::ErrorResp(payload) if is_revert(payload) => Self::Reverted,
            RpcError::ErrorResp(payload) => {
                Self::Unreached(format!("JSON-RPC error {}", payload.code))
            }
            other => Self::Unreached(failure(other).to_owned()),
        }
    }

    fn field<T>(self) -> Field<T> {
        match self {
            Self::Reverted => Field::Absent(Absence::Reverted),
            Self::Unreached(reason) => Field::Unreached(reason),
        }
    }
}

/// A `string`, or failing that the first word as a `bytes32`, which the MKR generation returns.
/// The fallback takes any decode failure: a left-padded `bytes32` reads as a plausible offset
/// and fails as an overrun rather than as a type mismatch.
fn label(data: &[u8]) -> Field<String> {
    if data.is_empty() {
        return Field::Absent(Absence::NoData);
    }

    let decoded = sol_data::String::abi_decode(data).ok().or_else(|| {
        data.get(..32)
            .map(|word| String::from_utf8_lossy(word).into_owned())
    });

    match decoded {
        Some(text) => sanitise(&text).map_or(Field::Absent(Absence::Blank), Field::Value),
        None => Field::Absent(Absence::Undecodable),
    }
}

/// The whole word, range-checked: a `uint8` decode masks high bits a token did return.
fn width(data: &[u8]) -> Field<u8> {
    if data.is_empty() {
        return Field::Absent(Absence::NoData);
    }

    match sol_data::Uint::<256>::abi_decode(data) {
        Ok(word) => u8::try_from(word).map_or(Field::Absent(Absence::OutOfRange), Field::Value),
        Err(_) => Field::Absent(Absence::Undecodable),
    }
}

fn sanitise(text: &str) -> Option<String> {
    let kept: String = text
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    // JavaScript's `trim`, which the stored labels went through, also strips U+FEFF; Rust's does not.
    let trimmed =
        kept.trim_matches(|character: char| character.is_whitespace() || character == '\u{feff}');
    if trimmed.is_empty() {
        return None;
    }

    let mut units = 0;
    Some(
        trimmed
            .chars()
            .take_while(|character| {
                units += character.len_utf16();
                units <= MAX_LABEL_UNITS
            })
            .collect(),
    )
}
