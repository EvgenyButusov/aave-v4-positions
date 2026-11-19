//! The document's own metadata, and what belongs in it.
//!
//! **The versioned API and nothing else.** The probes are absent deliberately —
//! [`ops::probe_router`] calls their paths infrastructure and [`crate::config`]
//! calls them no part of the versioned surface, so publishing them here would
//! contradict both. `docs/rust-migration.md` records the difference.
//!
//! The operations arrive with the routers that serve them, so a path cannot be
//! documented without being served.

use utoipa::OpenApi;

/// The version of the **API contract**, not of the package: a dependency bump
/// must not imply anything about the shape of the responses. It moves when a
/// shape, a default or a refusal does.
const CONTRACT_VERSION: &str = "0.3.0";

const DESCRIPTION: &str = "\
Indexed Aave v4 user positions, enriched and served for querying.

Positions in v4 have no token representation — no `aToken`, no `Transfer` \
events. They exist only as structs in Spoke storage, so everything served here \
is folded from the Spoke and Hub event streams rather than read back from chain \
state. Protocol details and the derivations behind each field are in \
`docs/aave-v4-protocol-analysis.md`.

**Integer amounts are strings.** Share balances exceed the 53-bit mantissa of a \
JSON number, and the failure mode of getting this wrong is a few wei of drift \
that reads as a rounding bug rather than a parse error.";

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Aave v4 Positions API",
        description = DESCRIPTION,
        version = CONTRACT_VERSION,
        // Written out because the derive falls back to `CARGO_PKG_LICENSE`,
        // empty here, and publishes `"license": {"name": ""}` rather than
        // nothing.
        license(name = "MIT"),
    ),
    tags((
        name = "positions",
        description = "Folded from the Spoke event stream, enriched from the Hub and the \
                       reserve registry. Balances are shares; `value` is what they are worth \
                       at the instant the page names, which defaults to the timestamp of the \
                       last indexed block that every response carries.",
    )),
)]
pub(crate) struct ApiDoc;
