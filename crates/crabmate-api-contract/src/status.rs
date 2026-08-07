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

#[cfg(test)]
mod golden {
    use super::*;
    use std::path::PathBuf;

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/status_shell_view_golden.json")
    }

    #[test]
    fn golden_status_shell_view_matches_fixture() {
        let path = fixture_path();
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let view: StatusShellView =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        assert_eq!(view.status, "ok");
        assert_eq!(view.model, "deepseek-chat");
        assert_eq!(view.default_session_mode, "act");
        assert_eq!(view.llm_context_tokens, 64_000);
        assert_eq!(
            view.tiktoken_new_session_baseline_by_agent_role
                .get("coder"),
            Some(&1500)
        );
        assert_eq!(
            view.tiktoken_new_session_baseline_by_agent_role.get(""),
            Some(&1200)
        );
        let round = serde_json::to_value(&view).expect("serialize");
        let again: StatusShellView = serde_json::from_value(round).expect("round-trip");
        assert_eq!(again.model, view.model);
        assert_eq!(
            again.tiktoken_new_session_baseline_by_agent_role,
            view.tiktoken_new_session_baseline_by_agent_role
        );
    }

    #[test]
    fn golden_status_shell_view_rejects_unknown_fields() {
        let path = fixture_path();
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let mut v: serde_json::Value =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        v.as_object_mut()
            .expect("object")
            .insert("unexpected_field".into(), serde_json::json!(1));
        assert!(
            serde_json::from_value::<StatusShellView>(v).is_err(),
            "StatusShellView must deny unknown fields"
        );
    }
}
