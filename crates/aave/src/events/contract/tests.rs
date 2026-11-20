use std::collections::HashSet;

use crate::events::Contract;

#[test]
fn follows_each_event_once_with_a_topic_of_its_own() {
    for (contract, followed) in [(Contract::Spoke, 8), (Contract::Hub, 13)] {
        let topics: HashSet<_> = contract.topics().into_iter().collect();

        assert_eq!(contract.events().len(), followed, "{contract}");
        assert_eq!(
            topics.len(),
            followed,
            "{contract}: a name missing from or overloaded in the ABI"
        );
    }
}

#[test]
fn shares_no_topic_between_the_spoke_and_the_hub() {
    let spoke: HashSet<_> = Contract::Spoke.topics().into_iter().collect();

    assert!(
        Contract::Hub
            .topics()
            .iter()
            .all(|topic| !spoke.contains(topic))
    );
    assert!(Contract::Spoke.follows("ReportDeficit") && Contract::Hub.follows("ReportDeficit"));
}

#[test]
fn leaves_out_the_hub_events_that_carry_no_asset_state() {
    for name in [
        "TransferShares",
        "AddSpoke",
        "UpdateSpokeConfig",
        "AuthorityUpdated",
    ] {
        assert!(
            Contract::Hub.abi().event(name).is_some(),
            "{name} is in the ABI"
        );
        assert!(!Contract::Hub.follows(name), "{name}");
    }
}
