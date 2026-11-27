//! The [`TokenMetadataStore`] port as an executable specification, which every implementation runs.

use std::collections::HashMap;

use alloy_primitives::{Address, address};
use units::ChainId;

use crate::{TokenLabel, TokenMetadataRow, TokenMetadataStore};

pub(crate) const CHAIN_ID: ChainId = ChainId::known(1);
pub(crate) const OTHER_CHAIN: ChainId = ChainId::known(8453);

pub(crate) const USDC: Address = address!("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");
pub(crate) const WETH: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
pub(crate) const UNASKED: Address = address!("6b175474e89094c44da98b954eedeac495271d0f");

/// Every field distinct from every other, so a crossed mapping cannot pass.
pub(crate) fn labelled(chain_id: ChainId, token: Address, symbol: &str) -> TokenMetadataRow {
    TokenMetadataRow {
        chain_id,
        token,
        symbol: Some(symbol.to_owned()),
        name: Some(format!("{symbol} Token")),
        token_decimals: Some(6),
        fetched_at_block: 25_652_782,
    }
}

/// A token that answered and had nothing to say, which EIP-20 allows.
fn mute(token: Address) -> TokenMetadataRow {
    TokenMetadataRow {
        symbol: None,
        name: None,
        token_decimals: None,
        ..labelled(CHAIN_ID, token, "")
    }
}

fn label(symbol: &str) -> TokenLabel {
    TokenLabel {
        symbol: Some(symbol.to_owned()),
        name: Some(format!("{symbol} Token")),
    }
}

pub(crate) trait Harness {
    type Store: TokenMetadataStore;

    /// A store of its own, named for the case, so cases can run at once.
    async fn fresh(case: &str) -> Self;

    fn store(&self) -> &Self::Store;
}

async fn given<H: Harness>(harness: &H, rows: &[TokenMetadataRow]) {
    harness
        .store()
        .put(rows)
        .await
        .expect("the rows should write");
}

async fn labels_of<H: Harness>(harness: &H, chain_id: ChainId) -> HashMap<Address, TokenLabel> {
    harness
        .store()
        .labels(chain_id)
        .await
        .expect("the dimension should read")
}

pub(crate) async fn reads_every_label_on_the_chain<H: Harness>() {
    let harness = H::fresh("reads_every_label_on_the_chain").await;
    given(
        &harness,
        &[
            labelled(CHAIN_ID, USDC, "USDC"),
            labelled(CHAIN_ID, WETH, "WETH"),
        ],
    )
    .await;

    assert_eq!(
        labels_of(&harness, CHAIN_ID).await,
        HashMap::from([(USDC, label("USDC")), (WETH, label("WETH"))])
    );
}

pub(crate) async fn keeps_one_chain_apart_from_another_listing_the_same_address<H: Harness>() {
    let harness = H::fresh("keeps_one_chain_apart").await;
    given(
        &harness,
        &[
            labelled(CHAIN_ID, USDC, "USDC"),
            labelled(OTHER_CHAIN, USDC, "USDbC"),
        ],
    )
    .await;

    assert_eq!(
        labels_of(&harness, CHAIN_ID).await,
        HashMap::from([(USDC, label("USDC"))])
    );
    assert_eq!(
        labels_of(&harness, OTHER_CHAIN).await,
        HashMap::from([(USDC, label("USDbC"))])
    );
}

/// A null label is an answer and a missing row is not, which is what stops a mute token being
/// re-read on every sweep.
pub(crate) async fn keeps_a_token_that_answered_with_nothing<H: Harness>() {
    let harness = H::fresh("keeps_a_token_that_answered_with_nothing").await;
    given(&harness, &[mute(USDC)]).await;

    assert_eq!(
        labels_of(&harness, CHAIN_ID).await,
        HashMap::from([(
            USDC,
            TokenLabel {
                symbol: None,
                name: None
            }
        )])
    );
}

pub(crate) async fn omits_a_token_with_no_row<H: Harness>() {
    let harness = H::fresh("omits_a_token_with_no_row").await;
    given(&harness, &[labelled(CHAIN_ID, USDC, "USDC")]).await;

    assert!(!labels_of(&harness, CHAIN_ID).await.contains_key(&UNASKED));
}

pub(crate) async fn replaces_a_token_read_again_rather_than_adding_to_it<H: Harness>() {
    let harness = H::fresh("replaces_a_token_read_again").await;
    given(&harness, &[labelled(CHAIN_ID, USDC, "USDC")]).await;
    given(&harness, &[labelled(CHAIN_ID, USDC, "USDC.e")]).await;

    assert_eq!(
        labels_of(&harness, CHAIN_ID).await,
        HashMap::from([(USDC, label("USDC.e"))])
    );
}

pub(crate) async fn writes_nothing_for_an_empty_batch<H: Harness>() {
    let harness = H::fresh("writes_nothing_for_an_empty_batch").await;

    given(&harness, &[]).await;

    assert!(labels_of(&harness, CHAIN_ID).await.is_empty());
}
