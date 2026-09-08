use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Private operator observation; never part of room history or participant state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ProviderUsage {
    pub provider_id: String,
    pub observed_at: DateTime<Utc>,
    pub quota: ProviderQuota,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderQuota {
    Balance {
        is_available: bool,
        balances: Vec<ProviderBalance>,
    },
}

/// Monetary decimals stay strings, preserving provider precision and denomination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ProviderBalance {
    pub currency: String,
    pub total_balance: String,
    pub granted_balance: String,
    pub topped_up_balance: String,
}
