//! 宿主侧观测钩子（turn replay、DSML 物化、日志脱敏等），供 [`super::complete_chat_retrying`] 注入。

use crabmate_types::Message;

/// 根 crate / 测试实现的 LLM 重试环侧效应（与 `crabmate-internal`、`runtime` 解耦）。
pub trait LlmRetryHooks: Send + Sync {
    fn append_turn_replay_json(&self, event: &str, model: &str, payload: Option<serde_json::Value>);

    fn append_decision_point(
        &self,
        phase: &str,
        decision_id: &str,
        outcome: &str,
        rationale: &str,
        detail: serde_json::Value,
        anchor_kind: &str,
        anchor: Option<serde_json::Value>,
    );

    fn materialize_dsml_tool_calls(&self, msg: &mut Message);

    fn assistant_preview_for_log(&self, msg: &Message) -> String;

    fn tool_arguments_preview_for_sse(&self, args: &str) -> String;

    fn assistant_content_for_log(&self, msg: &Message) -> String;

    fn reasoning_content_for_log(&self, msg: &Message) -> String;
}
