use std::str::FromStr;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

/// The name of the header used to indicate when a request failed due to an
/// expired LLM token.
///
/// The client may use this as a signal to refresh the token.
pub const EXPIRED_LLM_TOKEN_HEADER_NAME: &str = "x-zed-expired-token";

/// The name of the header used to indicate when a request failed due to an outdated LLM token.
///
/// A token is considered "outdated" when we can't parse the claims (e.g., after adding a new required claim).
///
/// This is distinct from [`EXPIRED_LLM_TOKEN_HEADER_NAME`] which indicates the token's time-based validity has passed.
/// An outdated token means the token's structure is incompatible with the current server expectations.
pub const OUTDATED_LLM_TOKEN_HEADER_NAME: &str = "x-zed-outdated-token";

/// The name of the header used to indicate the usage limit for edit predictions.
pub const EDIT_PREDICTIONS_USAGE_LIMIT_HEADER_NAME: &str = "x-zed-edit-predictions-usage-limit";

/// The name of the header used to indicate the usage amount for edit predictions.
pub const EDIT_PREDICTIONS_USAGE_AMOUNT_HEADER_NAME: &str = "x-zed-edit-predictions-usage-amount";

#[derive(Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageLimit {
    Limited(i32),
    Unlimited,
}

impl FromStr for UsageLimit {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "unlimited" => Ok(Self::Unlimited),
            limit => limit
                .parse::<i32>()
                .map(Self::Limited)
                .context("failed to parse limit"),
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct CurrentUsage {
    pub edit_predictions: UsageData,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageData {
    pub used: u32,
    pub limit: UsageLimit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_limit_from_str() {
        let limit = UsageLimit::from_str("unlimited").unwrap();
        assert!(matches!(limit, UsageLimit::Unlimited));

        let limit = UsageLimit::from_str(&0.to_string()).unwrap();
        assert!(matches!(limit, UsageLimit::Limited(0)));

        let limit = UsageLimit::from_str(&50.to_string()).unwrap();
        assert!(matches!(limit, UsageLimit::Limited(50)));

        for value in ["not_a_number", "50xyz"] {
            let limit = UsageLimit::from_str(value);
            assert!(limit.is_err());
        }
    }
}
