use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CheckRequest {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Valid,
    Invalid,
    Unknown,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Pan115,
    Pan189,
    Pan123,
    Generic,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pan115 => "pan115",
            Self::Pan189 => "pan189",
            Self::Pan123 => "pan123",
            Self::Generic => "generic",
        }
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CheckResult {
    pub original_url: String,
    pub normalized_url: String,
    pub status: CheckStatus,
    pub provider: Provider,
    pub reason: String,
    pub metadata: BTreeMap<String, Value>,
}

impl CheckResult {
    pub fn new(
        original_url: String,
        normalized_url: String,
        status: CheckStatus,
        provider: Provider,
        reason: impl Into<String>,
        metadata: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            original_url,
            normalized_url,
            status,
            provider,
            reason: reason.into(),
            metadata,
        }
    }
}
