use alloy_primitives::{B256, b256};
use alloy_sol_types::{SolCall as _, SolEvent as _};

use super::hub::IHubV4;
use super::oracle::IAaveOracleV4;
use super::spoke::ISpokeV4;

// Transcribed from docs/aave-v4-protocol-analysis.md §4, derived there from the Solidity interfaces
// at `2524fe4` and checked against mainnet logs: a second derivation the address book has to match.
type Catalogued = (&'static str, B256, &'static str, B256);

const SPOKE: &[Catalogued] = &[
    (
        ISpokeV4::Supply::SIGNATURE,
        ISpokeV4::Supply::SIGNATURE_HASH,
        "Supply(uint256,address,address,uint256,uint256)",
        b256!("0xd986db228cb1fe8392c5f45ff5f2c639b7db6cbd9ca7d1fe70b2de90c2c8c961"),
    ),
    (
        ISpokeV4::Borrow::SIGNATURE,
        ISpokeV4::Borrow::SIGNATURE_HASH,
        "Borrow(uint256,address,address,uint256,uint256)",
        b256!("0xef18174796a5d2f91d51dc5e907a4d7867bbd6e800f6225168e0453d581d0dcd"),
    ),
    (
        ISpokeV4::Withdraw::SIGNATURE,
        ISpokeV4::Withdraw::SIGNATURE_HASH,
        "Withdraw(uint256,address,address,uint256,uint256)",
        b256!("0xfe7813e2866053d5c3938554e517b554fce6666a6561bed9eaa7419b29fa9b68"),
    ),
    (
        ISpokeV4::Repay::SIGNATURE,
        ISpokeV4::Repay::SIGNATURE_HASH,
        "Repay(uint256,address,address,uint256,uint256,(int256,int256,uint256))",
        b256!("0xd765a0263e8a360da8dd4fdb8c0dc5553adec12a96f29a462cdb45e5bea407dd"),
    ),
    (
        ISpokeV4::LiquidationCall::SIGNATURE,
        ISpokeV4::LiquidationCall::SIGNATURE_HASH,
        "LiquidationCall(uint256,uint256,address,address,bool,uint256,uint256,(int256,int256,uint256),uint256,uint256,uint256)",
        b256!("0x2a1f12d996f530f89d8038aa293f9fde81cac44b6dfd6225e3358d09b78a4a37"),
    ),
    (
        ISpokeV4::SetUsingAsCollateral::SIGNATURE,
        ISpokeV4::SetUsingAsCollateral::SIGNATURE_HASH,
        "SetUsingAsCollateral(uint256,address,address,bool)",
        b256!("0x4763df430bc5274807f8ab4ce0734e7898513638418d6eec0c5285ef85f7f51f"),
    ),
    (
        ISpokeV4::ReportDeficit::SIGNATURE,
        ISpokeV4::ReportDeficit::SIGNATURE_HASH,
        "ReportDeficit(uint256,address,uint256,(int256,int256,uint256))",
        b256!("0x59932f333b3a5e3fec86e662babe8dd767529ed207420e7468bd220cdfb3f076"),
    ),
    (
        ISpokeV4::RefreshPremiumDebt::SIGNATURE,
        ISpokeV4::RefreshPremiumDebt::SIGNATURE_HASH,
        "RefreshPremiumDebt(uint256,address,(int256,int256,uint256))",
        b256!("0x4fd0c5440d5b8c1dd712c65f039f54384c59e81a139427b0a9155260d974a9a7"),
    ),
    (
        ISpokeV4::RefreshAllUserDynamicConfig::SIGNATURE,
        ISpokeV4::RefreshAllUserDynamicConfig::SIGNATURE_HASH,
        "RefreshAllUserDynamicConfig(address)",
        b256!("0x837314749a8459031ad895d39a13552d1627fddc93d64b404bab0ae5f0798da7"),
    ),
    (
        ISpokeV4::RefreshSingleUserDynamicConfig::SIGNATURE,
        ISpokeV4::RefreshSingleUserDynamicConfig::SIGNATURE_HASH,
        "RefreshSingleUserDynamicConfig(address,uint256)",
        b256!("0x5790b5f096c9cfee6b98a4e2d4f54ff3fc4ca306df5bc2093d93a36496d917b8"),
    ),
    (
        ISpokeV4::SetUserPositionManager::SIGNATURE,
        ISpokeV4::SetUserPositionManager::SIGNATURE_HASH,
        "SetUserPositionManager(address,address,bool)",
        b256!("0x413bea992b9956f4f10f6c819bf7a6c8ed5baa119a2901fe221ae03171d52277"),
    ),
    (
        ISpokeV4::UpdateUserRiskPremium::SIGNATURE,
        ISpokeV4::UpdateUserRiskPremium::SIGNATURE_HASH,
        "UpdateUserRiskPremium(address,uint256)",
        b256!("0x9a9082fd74a00ac52b567642a2d8fd3383cb2bd8690f6b2a3b7b37aaf489dac1"),
    ),
    (
        ISpokeV4::AddReserve::SIGNATURE,
        ISpokeV4::AddReserve::SIGNATURE_HASH,
        "AddReserve(uint256,uint256,address)",
        b256!("0xb2d3221c3db1eb0d586556ae23399acdfe3e52ff0fcd184c19069c730f9ca2e9"),
    ),
    (
        ISpokeV4::UpdateReservePriceSource::SIGNATURE,
        ISpokeV4::UpdateReservePriceSource::SIGNATURE_HASH,
        "UpdateReservePriceSource(uint256,address)",
        b256!("0x18a45d070f507b6387b78837652d7468e733927acc7f9a13d9cc308675735c08"),
    ),
    (
        ISpokeV4::UpdateReserveConfig::SIGNATURE,
        ISpokeV4::UpdateReserveConfig::SIGNATURE_HASH,
        "UpdateReserveConfig(uint256,(uint24,bool,bool,bool,bool))",
        b256!("0xe9495512a0eb05fe0cbdd52286bdeb54cb8e5a8d50e7e17d75f75903a98e2af8"),
    ),
    (
        ISpokeV4::AddDynamicReserveConfig::SIGNATURE,
        ISpokeV4::AddDynamicReserveConfig::SIGNATURE_HASH,
        "AddDynamicReserveConfig(uint256,uint32,(uint16,uint32,uint16))",
        b256!("0xfcede5501ba87e3766118ae6ed360a87ee9b6570156ae9cac52d35ff0de0403b"),
    ),
    (
        ISpokeV4::UpdateDynamicReserveConfig::SIGNATURE,
        ISpokeV4::UpdateDynamicReserveConfig::SIGNATURE_HASH,
        "UpdateDynamicReserveConfig(uint256,uint32,(uint16,uint32,uint16))",
        b256!("0x2d4f2760aaff0dfa53526a8fdd306864689a7d5e43f44ddfeece0f38315c298d"),
    ),
    (
        ISpokeV4::UpdatePositionManager::SIGNATURE,
        ISpokeV4::UpdatePositionManager::SIGNATURE_HASH,
        "UpdatePositionManager(address,bool)",
        b256!("0x8e04e916c2b397f8ab1cf9a55e94728a44837b3751f72369339ad991d371edc4"),
    ),
    (
        ISpokeV4::UpdateLiquidationConfig::SIGNATURE,
        ISpokeV4::UpdateLiquidationConfig::SIGNATURE_HASH,
        "UpdateLiquidationConfig((uint128,uint64,uint16))",
        b256!("0x9062eec1933c38394d82dc926d7ddcd777a5cd08e1ae6baa94e90047338d3459"),
    ),
    (
        ISpokeV4::SetSpokeImmutables::SIGNATURE,
        ISpokeV4::SetSpokeImmutables::SIGNATURE_HASH,
        "SetSpokeImmutables(address,uint16)",
        b256!("0x6d87c7e547bc13244d61719fa011b6947b26036a16d69a607c1cf72a77d052bc"),
    ),
];

const HUB: &[Catalogued] = &[
    (
        IHubV4::UpdateAsset::SIGNATURE,
        IHubV4::UpdateAsset::SIGNATURE_HASH,
        "UpdateAsset(uint256,uint256,uint256,uint256)",
        b256!("0xa1facf110ded5028ee267fa3d5986f2aa4dc14230b79ffd27e95760f14883350"),
    ),
    (
        IHubV4::Add::SIGNATURE,
        IHubV4::Add::SIGNATURE_HASH,
        "Add(uint256,address,uint256,uint256)",
        b256!("0xb233dd05ed21346e144167b35a6213bcf04768dbdffdc8339e8b027b94b9f305"),
    ),
    (
        IHubV4::Remove::SIGNATURE,
        IHubV4::Remove::SIGNATURE_HASH,
        "Remove(uint256,address,uint256,uint256)",
        b256!("0x535be2ff85ab4c5d0991e10dc057a4951ea2bac426ffb036eded23036a3942b2"),
    ),
    (
        IHubV4::Draw::SIGNATURE,
        IHubV4::Draw::SIGNATURE_HASH,
        "Draw(uint256,address,uint256,uint256)",
        b256!("0xe2497bc41b1fa7c4ba996f24dc2affdffb2a5571584db6db0eed8fbbf1dc8517"),
    ),
    (
        IHubV4::Restore::SIGNATURE,
        IHubV4::Restore::SIGNATURE_HASH,
        "Restore(uint256,address,uint256,(int256,int256,uint256),uint256,uint256)",
        b256!("0x119e7f996dc987b3ae79eb3735f1620c4292f6a7761a1e0f834c445f7798b912"),
    ),
    (
        IHubV4::RefreshPremium::SIGNATURE,
        IHubV4::RefreshPremium::SIGNATURE_HASH,
        "RefreshPremium(uint256,address,(int256,int256,uint256))",
        b256!("0x3fa96ecf17429fddfbb919a64196f4e43f71b57f0c5c38c49a21c8e1e763d18c"),
    ),
    (
        IHubV4::ReportDeficit::SIGNATURE,
        IHubV4::ReportDeficit::SIGNATURE_HASH,
        "ReportDeficit(uint256,address,uint256,(int256,int256,uint256),uint256)",
        b256!("0x4845ee5c72bde2b62defc8a1ca2f0fc3313b2d9e799997ce4f6776da9773bcbf"),
    ),
    (
        IHubV4::EliminateDeficit::SIGNATURE,
        IHubV4::EliminateDeficit::SIGNATURE_HASH,
        "EliminateDeficit(uint256,address,address,uint256,uint256)",
        b256!("0xe97b8576ac531cdc817b933309d0518ca3d26c6b46d490f3ae9fa39426a141ee"),
    ),
    (
        IHubV4::MintFeeShares::SIGNATURE,
        IHubV4::MintFeeShares::SIGNATURE_HASH,
        "MintFeeShares(uint256,address,uint256,uint256)",
        b256!("0xafd21228e21de4a3f779e1cc3617e12672c3da091dcf3812a931036aa0bf633c"),
    ),
    (
        IHubV4::Sweep::SIGNATURE,
        IHubV4::Sweep::SIGNATURE_HASH,
        "Sweep(uint256,address,uint256)",
        b256!("0x69bb3893073d7a893f3933f3871309fc25acfc72e365b71f554d439a85b20e8b"),
    ),
    (
        IHubV4::Reclaim::SIGNATURE,
        IHubV4::Reclaim::SIGNATURE_HASH,
        "Reclaim(uint256,address,uint256)",
        b256!("0x566111831db1f090374baff3c3f9fc512084f5a9b8f5b199fb475d9c43a8013f"),
    ),
    (
        IHubV4::TransferShares::SIGNATURE,
        IHubV4::TransferShares::SIGNATURE_HASH,
        "TransferShares(uint256,address,address,uint256)",
        b256!("0x0d93b0e8579bc9db73c85a1fb79d785ffc47f8e20d346253f809cc98c48292a0"),
    ),
];

#[test]
fn derives_every_spoke_topic_the_protocol_analysis_derived() {
    for &(signature, hash, catalogued, catalogued_hash) in SPOKE {
        assert_eq!((signature, hash), (catalogued, catalogued_hash));
    }
}

#[test]
fn derives_every_hub_topic_the_protocol_analysis_derived() {
    for &(signature, hash, catalogued, catalogued_hash) in HUB {
        assert_eq!((signature, hash), (catalogued, catalogued_hash));
    }
}

#[test]
fn keeps_the_hubs_report_deficit_apart_from_the_spokes() {
    assert_ne!(
        IHubV4::ReportDeficit::SIGNATURE_HASH,
        ISpokeV4::ReportDeficit::SIGNATURE_HASH
    );
}

#[test]
fn prices_every_reserve_of_a_spoke_in_one_call() {
    assert_eq!(
        IAaveOracleV4::getReservesPricesCall::SIGNATURE,
        "getReservesPrices(uint256[])"
    );
}
