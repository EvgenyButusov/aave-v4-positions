mod harness;

use serde_json::Value;

use super::row::Row;
use crate::events::fixtures::{decoder, event, golden};
use crate::events::{ClickHouseEventLedger, Contract, Error};

use harness::{Ledgers, at, empty_database, live, raw};

#[test]
fn writes_each_captured_log_as_the_row_the_typescript_wrote_for_it() {
    for contract in [Contract::Spoke, Contract::Hub] {
        for (log, written) in golden(contract) {
            let event = decoder(contract).decode(&[log]).unwrap().remove(0);

            let mut row = serde_json::to_value(Row::live(&event, 0)).unwrap();
            let fields = row.as_object_mut().unwrap();
            fields.remove("version");
            fields.remove("sign");

            assert_eq!(&row, written, "{contract} {}", event.event_name);
        }
    }
}

#[tokio::test]
async fn reads_back_a_range_once_however_often_it_is_indexed() {
    let ledgers = Ledgers::new("indexed_twice").await;
    let batch = [at(event(Contract::Spoke, "Supply"), 1_000, 0)];

    ledgers.index(Contract::Spoke, 1_000, 1_000, &batch).await;
    ledgers.index(Contract::Spoke, 1_000, 1_000, &batch).await;

    assert_eq!(live(&ledgers, Contract::Spoke).await.len(), 1);
}

#[tokio::test]
async fn doubles_a_range_appended_again_without_the_revert_before_it() {
    let ledgers = Ledgers::new("appended_twice").await;
    let batch = [at(event(Contract::Spoke, "Supply"), 1_000, 0)];

    ledgers.spoke.append(&batch).await.unwrap();
    ledgers.spoke.append(&batch).await.unwrap();

    assert_eq!(live(&ledgers, Contract::Spoke).await.len(), 2);
}

#[tokio::test]
async fn empties_a_range_given_no_events_which_is_what_a_reorg_does() {
    let ledgers = Ledgers::new("reorged_away").await;
    ledgers
        .index(
            Contract::Spoke,
            1_000,
            1_001,
            &[
                at(event(Contract::Spoke, "Supply"), 1_000, 0),
                at(event(Contract::Spoke, "Borrow"), 1_001, 0),
            ],
        )
        .await;

    ledgers.index(Contract::Spoke, 1_000, 1_001, &[]).await;

    assert!(live(&ledgers, Contract::Spoke).await.is_empty());
}

#[tokio::test]
async fn retracts_only_the_blocks_it_is_given_both_ends_inclusive() {
    let ledgers = Ledgers::new("bounded").await;
    let supply = event(Contract::Spoke, "Supply");
    ledgers
        .index(
            Contract::Spoke,
            999,
            1_002,
            &[
                at(supply.clone(), 999, 0),
                at(supply.clone(), 1_000, 0),
                at(supply.clone(), 1_001, 0),
                at(supply, 1_002, 0),
            ],
        )
        .await;

    ledgers
        .spoke
        .revert(ledgers.chain_id, 1_000, 1_001)
        .await
        .unwrap();

    let blocks: Vec<u64> = live(&ledgers, Contract::Spoke)
        .await
        .iter()
        .map(|row| row.block_number)
        .collect();
    assert_eq!(blocks, [999, 1_002]);
}

#[tokio::test]
async fn retracts_only_the_chain_it_is_given() {
    let ledgers = Ledgers::new("one_chain").await;
    let mut elsewhere = at(event(Contract::Spoke, "Supply"), 1_000, 0);
    elsewhere.chain_id = units::ChainId::known(11_297_108_109);
    ledgers.spoke.append(&[elsewhere]).await.unwrap();

    ledgers
        .spoke
        .revert(ledgers.chain_id, 1_000, 1_000)
        .await
        .unwrap();

    assert_eq!(raw(&ledgers, Contract::Spoke).await.len(), 1);
}

#[tokio::test]
async fn drops_the_surplus_when_a_reorged_block_holds_fewer_logs() {
    let ledgers = Ledgers::new("fewer_logs").await;
    let supply = event(Contract::Spoke, "Supply");
    ledgers
        .index(
            Contract::Spoke,
            1_000,
            1_000,
            &[at(supply.clone(), 1_000, 0), at(supply.clone(), 1_000, 1)],
        )
        .await;

    ledgers
        .index(Contract::Spoke, 1_000, 1_000, &[at(supply, 1_000, 0)])
        .await;

    let logs: Vec<u32> = live(&ledgers, Contract::Spoke)
        .await
        .iter()
        .map(|row| row.log_index)
        .collect();
    assert_eq!(logs, [0]);
}

#[tokio::test]
async fn replaces_what_a_reorg_rewrote_at_the_same_block_and_log_index() {
    let ledgers = Ledgers::new("rewritten").await;
    ledgers
        .index(
            Contract::Spoke,
            1_000,
            1_000,
            &[at(event(Contract::Spoke, "Supply"), 1_000, 0)],
        )
        .await;

    ledgers
        .index(
            Contract::Spoke,
            1_000,
            1_000,
            &[at(event(Contract::Spoke, "Borrow"), 1_000, 0)],
        )
        .await;

    let names: Vec<String> = live(&ledgers, Contract::Spoke)
        .await
        .into_iter()
        .map(|row| row.event_name)
        .collect();
    assert_eq!(names, ["Borrow"]);
}

#[tokio::test]
async fn retracts_with_an_exact_copy_of_every_column_of_the_row() {
    let ledgers = Ledgers::new("exact_copy").await;
    ledgers
        .index(
            Contract::Spoke,
            1_000,
            1_000,
            &[at(event(Contract::Spoke, "Repay"), 1_000, 0)],
        )
        .await;

    ledgers
        .spoke
        .revert(ledgers.chain_id, 1_000, 1_000)
        .await
        .unwrap();

    let rows = raw(&ledgers, Contract::Spoke).await;
    let (live, retraction): (Vec<_>, Vec<_>) = rows.into_iter().partition(|row| row["sign"] == 1);
    let without_sign = |mut row: Value| {
        row.as_object_mut().unwrap().remove("sign");
        row
    };
    assert_eq!(
        live.into_iter().map(without_sign).collect::<Vec<_>>(),
        retraction.into_iter().map(without_sign).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn keeps_a_number_past_two_to_the_fifty_third_digit_for_digit() {
    let ledgers = Ledgers::new("exact_digits").await;
    let checkpoint = at(event(Contract::Hub, "UpdateAsset"), 1_000, 0);

    ledgers
        .index(
            Contract::Hub,
            1_000,
            1_000,
            std::slice::from_ref(&checkpoint),
        )
        .await;

    let stored = live(&ledgers, Contract::Hub).await.remove(0);
    assert_eq!(stored.body, checkpoint.body);
}

#[tokio::test]
async fn keeps_the_hub_and_spoke_ledgers_apart_over_the_same_blocks() {
    let ledgers = Ledgers::new("apart").await;
    ledgers
        .index(
            Contract::Spoke,
            1_000,
            1_000,
            &[at(event(Contract::Spoke, "ReportDeficit"), 1_000, 0)],
        )
        .await;
    ledgers
        .index(
            Contract::Hub,
            1_000,
            1_000,
            &[at(event(Contract::Hub, "ReportDeficit"), 1_000, 1)],
        )
        .await;

    ledgers
        .hub
        .revert(ledgers.chain_id, 1_000, 1_000)
        .await
        .unwrap();

    assert!(live(&ledgers, Contract::Hub).await.is_empty());
    assert_eq!(live(&ledgers, Contract::Spoke).await.len(), 1);
}

#[tokio::test]
async fn names_the_ledger_and_the_range_clickhouse_refused() {
    let ledger = ClickHouseEventLedger::new(empty_database("unmigrated").await, Contract::Hub);
    let chain_id = crate::MAIN_SPOKE_ON_ETHEREUM.chain_id;

    let reverted = ledger.revert(chain_id, 1_000, 1_001).await.unwrap_err();
    let appended = ledger
        .append(&[event(Contract::Hub, "Add")])
        .await
        .unwrap_err();

    assert!(matches!(
        reverted,
        Error::RevertFailed {
            from: 1_000,
            to: 1_001,
            ..
        }
    ));
    assert_eq!(
        reverted.to_string(),
        "the hub ledger could not retract blocks 1000..=1001"
    );
    assert!(matches!(appended, Error::AppendFailed { events: 1, .. }));
}
