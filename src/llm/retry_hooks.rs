//! 宿主侧 [`crabmate_llm::LlmRetryHooks`] 实现。

use crabmate_config::AgentConfig;
use crabmate_llm::{LlmRetryDecisionPoint, LlmRetryHooks};
use crabmate_types::{Message, message_content_as_str};

pub struct CrabmateLlmRetryHooks<'a> {
    pub cfg: &'a AgentConfig,
}

impl LlmRetryHooks for CrabmateLlmRetryHooks<'_> {
    fn append_turn_replay_json(
        &self,
        event: &str,
        model: &str,
        payload: Option<serde_json::Value>,
    ) {
        let detail = payload.as_ref();
        crate::turn_replay_dump::append_turn_replay_event_json_if_configured(event, model, detail);
    }

    fn append_decision_point(&self, decision: &LlmRetryDecisionPoint) {
        crate::turn_replay_dump::append_decision_point_event_if_configured(
            &decision.phase,
            &decision.decision_id,
            &decision.outcome,
            &decision.rationale,
            decision.detail.clone(),
            &decision.anchor_kind,
            decision.anchor.clone(),
        );
    }

    fn materialize_dsml_tool_calls(&self, msg: &mut Message) {
        crate::dsml::materialize_deepseek_dsml_tool_calls_in_message(
            msg,
            self.cfg
                .dsml_materialize
                .materialize_deepseek_dsml_tool_calls,
        );
    }

    fn assistant_preview_for_log(&self, msg: &Message) -> String {
        crate::redact::assistant_message_preview_for_log(msg)
    }

    fn tool_arguments_preview_for_sse(&self, args: &str) -> String {
        crate::redact::tool_arguments_preview_for_sse(args)
    }

    fn assistant_content_for_log(&self, msg: &Message) -> String {
        message_content_as_str(&msg.content)
            .unwrap_or("")
            .to_string()
    }

    fn reasoning_content_for_log(&self, msg: &Message) -> String {
        msg.reasoning_content.clone().unwrap_or_default()
    }
}
