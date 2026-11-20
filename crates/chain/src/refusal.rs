//! Whether a provider's refusal means "ask for fewer blocks".
//!
//! Read off the message, because neither the error code nor the HTTP status tells the cases
//! apart: two providers both answer `-32602`, one meaning the range is too wide and the other that
//! the plan has no history, and narrowing for the second would never help. A phrasing this misses
//! is retried at the same width, which makes no progress and corrupts nothing.

use alloy_transport::{RpcError, TransportError, TransportErrorKind};

const SUBJECTS: [&str; 4] = ["range", "ranges", "result", "results"];
const LIMITS: [&str; 6] = ["exceed", "exceeds", "exceeded", "maximum", "max", "limit"];
const LIMIT_PHRASES: [[&str; 2]; 4] = [
    ["too", "large"],
    ["too", "many"],
    ["not", "supported"],
    ["limited", "to"],
];

/// The provider's own words, wherever this error carries them.
pub(crate) fn detail(error: &TransportError) -> Option<&str> {
    match error {
        RpcError::ErrorResp(payload) => Some(&payload.message),
        RpcError::Transport(
            TransportErrorKind::HttpError(http)
            | TransportErrorKind::HttpErrorWithRetryAfter { error: http, .. },
        ) => Some(&http.body),
        _ => None,
    }
}

pub(crate) fn is_range_rejection(detail: &str) -> bool {
    let lowered = detail.to_lowercase();
    let words: Vec<&str> = lowered
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();

    let names_a_range = words.iter().any(|word| SUBJECTS.contains(word));
    let names_a_limit = words.iter().any(|word| LIMITS.contains(word))
        || words
            .windows(2)
            .any(|pair| LIMIT_PHRASES.iter().any(|phrase| pair == phrase));

    names_a_range && names_a_limit
}

#[cfg(test)]
mod tests {
    use super::is_range_rejection;

    #[test]
    fn recognises_every_range_refusal_measured_against_a_public_provider() {
        for refusal in [
            "ranges over 10000 blocks are not supported on free plan",
            "eth_getLogs is limited to 0 - 50 blocks range",
            "Block range too large: maximum allowed is 50 blocks",
            "Log response size exceeded. You can make eth_getLogs requests with up to a 2K block range",
        ] {
            assert!(is_range_rejection(refusal), "{refusal}");
        }
    }

    #[test]
    fn does_not_narrow_for_a_plan_that_has_no_history() {
        assert!(!is_range_rejection(
            "Archive requests require a personal token, get one at https://publicnode.com"
        ));
    }

    #[test]
    fn needs_both_a_range_and_a_limit() {
        assert!(!is_range_rejection("rate limit exceeded"));
        assert!(!is_range_rejection("block range"));
    }

    #[test]
    fn matches_whole_words_only() {
        assert!(!is_range_rejection("arrangement limited to members"));
        assert!(!is_range_rejection("range is unlimited"));
    }
}
