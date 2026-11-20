use alloy_primitives::Address;
use serde_json::Value;

use crate::MAIN_SPOKE_ON_ETHEREUM;

#[test]
fn matches_the_deployment_the_address_book_publishes() {
    let published: Value = serde_json::from_str(include_str!("../../abi/deployment.json")).unwrap();
    let address = |key: &str| published[key].as_str().unwrap().parse::<Address>().unwrap();

    assert_eq!(address("mainSpoke"), MAIN_SPOKE_ON_ETHEREUM.spoke);
    assert_eq!(
        address("mainSpokeOracle"),
        MAIN_SPOKE_ON_ETHEREUM.spoke_oracle
    );
    assert_eq!(address("coreHub"), MAIN_SPOKE_ON_ETHEREUM.hub);
}
