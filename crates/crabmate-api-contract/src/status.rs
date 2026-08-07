//! `GET /status?view=shell` 响应：Web 壳层所需字段的稳定子集。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Web UI / 状态栏 / 设置页消费的 `/status` 视图（`?view=shell`）。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusShellView {
    pub status: String,
    pub model: String,
    pub api_base: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_role_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_agent_role_id: Option<String>,
    #[serde(default = "default_session_mode_act")]
    pub default_session_mode: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub agent_role_default_session_modes: BTreeMap<String, String>,
    #[serde(default)]
    pub context_char_budget: usize,
    #[serde(default)]
    pub llm_context_tokens: u32,
    #[serde(default)]
    pub effective_context_char_budget: usize,
    #[serde(default)]
    pub tiktoken_prompt_counting_model: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tiktoken_new_session_baseline_by_agent_role: BTreeMap<String, u32>,
    #[serde(default)]
    pub executor_model: String,
    #[serde(default)]
    pub executor_api_base: String,
    #[serde(default)]
    pub planner_executor_mode: String,
    #[serde(default)]
    pub conversation_store_sqlite_path_configured: bool,
    #[serde(default)]
    pub conversation_store_sqlite_active: bool,
}

fn default_session_mode_act() -> String {
    "act".to_string()
}

impl StatusShellView {
    pub fn ok_prefix() -> &'static str {
        "ok"
    }
}
