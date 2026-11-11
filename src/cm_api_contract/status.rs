//! `GET /status?view=shell` 响应：Web 壳层所需字段的稳定子集。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Web UI / 状态栏 / 设置页消费的 `/status` 视图（`?view=shell`）。
///
/// **响应体，刻意不 `deny_unknown_fields`**（#1347 S3b）：壳层前端把本类型直接当 `StatusData`
/// 反序列化，若这里拒绝未知字段，Server 每加一个字段都会让未升级的 Client **解析失败**
/// （`/status` 整块拿不到）。入站宽容 + 出站由 golden 逐字段断言守住，是响应体应有的取舍；
/// **请求体**（`workspace.rs` / `api.rs` 的 `*Body`）保持 `deny_unknown_fields`，那是为了拒拼写错误。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
    /// 进程默认权限等级（`permission_level`：`chat` / `read` / `workspace` / `approved` / `unrestricted`）。
    ///
    /// 只放**两个标量**：六轴投影不进壳层视图（设计 §16.2「不塞进 `StatusShellView`」），
    /// 要看轴名用 `self_config_info` 的 `permission` 小节。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub permission_level_default: String,
    /// 启动期冻结的生效上限（`permission_level_max_effective`）；客户端据此解释「为什么不能选 L4」。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub permission_level_max: String,
    /// 输入预算快照（#1347 S3b）：供 Client 取「用量 / 预算」的分母。
    ///
    /// 与运行期预算、doctor 同源于 `cm_config::context_gates`；未配置 `llm_context_tokens`
    /// （走字符预算降级路径）时缺省。**不含**回合实测用量——那走 `context_trim` / SSE。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_budget: Option<crate::cm_types::ContextBudgetView>,
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
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/status_shell_view_golden.json")
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
        // #1347 S3b：预算快照随壳层视图一起发布，供 Client 取分母（不含回合实测用量）。
        assert_eq!(
            view.context_budget,
            Some(crate::cm_types::ContextBudgetView {
                context_window_tokens: 64_000,
                max_input_tokens: 45_568,
                trigger_tokens: 38_732,
                target_tokens: 31_897,
            })
        );
        let round = serde_json::to_value(&view).expect("serialize");
        let again: StatusShellView = serde_json::from_value(round).expect("round-trip");
        assert_eq!(again.model, view.model);
        assert_eq!(
            again.tiktoken_new_session_baseline_by_agent_role,
            view.tiktoken_new_session_baseline_by_agent_role
        );
    }

    /// #1347 S3b：壳层视图是**响应体**，必须**容忍**未知字段——旧 Client 读到新 Server 加上的
    /// 字段要忽略而不是解析失败（否则 Server 每加一个字段都会让 `/status` 整块拿不到）。
    /// 「出站不漂移」由上面 fixture 的逐字段断言与往返断言守住；请求体仍各自 `deny_unknown_fields`。
    #[test]
    fn golden_status_shell_view_tolerates_unknown_fields() {
        let path = fixture_path();
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let mut v: serde_json::Value =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        let obj = v.as_object_mut().expect("object");
        obj.insert("unexpected_top_level".into(), serde_json::json!(1));
        obj.get_mut("context_budget")
            .and_then(|b| b.as_object_mut())
            .expect("fixture 应带 context_budget")
            .insert("unexpected_nested".into(), serde_json::json!(true));

        let view: StatusShellView = serde_json::from_value(v).expect("未知字段必须被忽略");
        assert_eq!(view.model, "deepseek-chat");
        assert_eq!(
            view.context_budget.map(|b| b.max_input_tokens),
            Some(45_568),
            "已知字段仍要正确解析"
        );
    }
}
