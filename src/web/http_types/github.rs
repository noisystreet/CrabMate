//! GitHub 在线模式 HTTP JSON 契约。

use serde::{Deserialize, Serialize};

pub use crate::tools::web_api::{GithubPrCurrentChecksData, GithubPrsData, GithubRepoContextData};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubRepoContextResponse {
    #[serde(flatten)]
    pub data: GithubRepoContextData,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubPrsResponse {
    #[serde(flatten)]
    pub data: GithubPrsData,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubPrCurrentChecksResponse {
    #[serde(flatten)]
    pub data: GithubPrCurrentChecksData,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GithubPrsQuery {
    pub limit: Option<u32>,
}
