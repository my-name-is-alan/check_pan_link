use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CheckRequest {
    pub url: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Pan115ListType {
    #[default]
    Files,
    Tree,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Pan115ShareListRequest {
    pub url: String,
    #[serde(default)]
    pub list_type: Pan115ListType,
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Pan115ShareFile {
    pub fid: String,
    pub parent_cid: String,
    pub name: String,
    pub path: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Pan115ShareFolderNode {
    pub cid: String,
    pub name: String,
    pub path: String,
    pub children: Vec<Pan115ShareNode>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "node_type", rename_all = "snake_case")]
pub enum Pan115ShareNode {
    Folder {
        cid: String,
        name: String,
        path: String,
        children: Vec<Pan115ShareNode>,
    },
    File {
        fid: String,
        parent_cid: String,
        name: String,
        path: String,
        size: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        sha1: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        extension: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "result_type", rename_all = "snake_case")]
pub enum Pan115ShareListPayload {
    Files { files: Vec<Pan115ShareFile> },
    Tree { tree: Pan115ShareFolderNode },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Pan115ShareListResponse {
    pub original_url: String,
    pub normalized_url: String,
    pub provider: Provider,
    pub list_type: Pan115ListType,
    pub share_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receive_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share_state: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share_state_label: Option<String>,
    pub file_count: usize,
    #[serde(flatten)]
    pub payload: Pan115ShareListPayload,
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
