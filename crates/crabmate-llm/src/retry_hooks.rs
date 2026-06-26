//! 宿主侧观测钩子（turn replay、DSML 物化、日志脱敏等），供 [`super::complete_chat_retrying`] 注入。

use crabmate_types::Message;

/// turn replay `decision_point` 事件字段（对齐根包 `append_decision_point_event_if_configured`）。
#[derive(Debug, Clone)]
pub struct LlmRetryDecisionPoint {
    pub phase: String,
    pub decision_id: String,
    pub outcome: String,
    pub rationale: String,
    pub detail: serde_json::Value,
    pub anchor_kind: String,
    pub anchor: Option<serde_json::Value>,
}

/// 根 crate / 测试实现的 LLM 重试环侧效应（与 `crabmate-internal`、`runtime` 解耦）。
pub trait LlmRetryHooks: Send + Sync {
    fn append_turn_replay_json(&self, event: &str, model: &str, payload: Option<serde_json::Value>);

    fn append_decision_point(&self, decision: &LlmRetryDecisionPoint);

    fn materialize_dsml_tool_calls(&self, msg: &mut Message);

    fn assistant_preview_for_log(&self, msg: &Message) -> String;

    fn tool_arguments_preview_for_sse(&self, args: &str) -> String;

    fn assistant_content_for_log(&self, msg: &Message) -> String;

    fn reasoning_content_for_log(&self, msg: &Message) -> String;
}
