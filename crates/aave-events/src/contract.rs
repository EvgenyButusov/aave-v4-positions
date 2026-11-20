#[cfg(test)]
mod tests;

use std::fmt;
use std::sync::LazyLock;

use alloy_json_abi::{Event, JsonAbi};
use alloy_primitives::B256;

/// A contract whose events are followed, and so which ABI decodes them and which ledger keeps them.
/// The two never share a ledger: both emit a `ReportDeficit`, with different signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contract {
    Spoke,
    Hub,
}

// The events that move a position, and `AddReserve`, which gives `reserveId` a meaning. The
// position managers' mirror events report actions the Spoke already reports; folding them as well
// would count each routed action twice.
const SPOKE_EVENTS: [&str; 8] = [
    "Supply",
    "Withdraw",
    "Borrow",
    "Repay",
    "LiquidationCall",
    "ReportDeficit",
    "SetUsingAsCollateral",
    "AddReserve",
];

// The events that move Hub asset state. `TransferShares` moves shares between spokes and so nets to
// zero for an asset; `AddSpoke`, `UpdateSpokeConfig` and `AuthorityUpdated` carry none.
const HUB_EVENTS: [&str; 13] = [
    "Add",
    "Remove",
    "Draw",
    "Restore",
    "MintFeeShares",
    "Sweep",
    "Reclaim",
    "ReportDeficit",
    "EliminateDeficit",
    "RefreshPremium",
    "UpdateAsset",
    "UpdateAssetConfig",
    "AddAsset",
];

#[expect(
    clippy::expect_used,
    reason = "the vendored ABI is a compile-time constant its own tests parse; a panic is the report"
)]
fn parse(json: &str) -> JsonAbi {
    serde_json::from_str(json).expect("a vendored ABI that parses")
}

static SPOKE_ABI: LazyLock<JsonAbi> = LazyLock::new(|| parse(aave_abi::spoke::ABI_JSON));
static HUB_ABI: LazyLock<JsonAbi> = LazyLock::new(|| parse(aave_abi::hub::ABI_JSON));

impl Contract {
    #[must_use]
    pub fn events(self) -> &'static [&'static str] {
        match self {
            Self::Spoke => &SPOKE_EVENTS,
            Self::Hub => &HUB_EVENTS,
        }
    }

    /// Each followed event's `topic0`, derived from the ABI: a mistyped hash would match nothing,
    /// which looks exactly like a quiet chain.
    #[must_use]
    pub fn topics(self) -> Vec<B256> {
        self.events()
            .iter()
            .filter_map(|name| self.abi().event(name))
            .flatten()
            .map(Event::selector)
            .collect()
    }

    pub(crate) fn abi(self) -> &'static JsonAbi {
        match self {
            Self::Spoke => &SPOKE_ABI,
            Self::Hub => &HUB_ABI,
        }
    }

    pub(crate) fn follows(self, event: &str) -> bool {
        self.events().contains(&event)
    }

    pub(crate) fn table(self) -> &'static str {
        match self {
            Self::Spoke => "spoke_events",
            Self::Hub => "hub_events",
        }
    }

    /// Every read goes through this: an unpaired retraction is still visible in the table.
    pub(crate) fn view(self) -> &'static str {
        match self {
            Self::Spoke => "spoke_events_current",
            Self::Hub => "hub_events_current",
        }
    }
}

impl fmt::Display for Contract {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Spoke => "spoke",
            Self::Hub => "hub",
        })
    }
}
