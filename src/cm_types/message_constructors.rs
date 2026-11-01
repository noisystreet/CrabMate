//! `Message` 的具名构造器（`impl` 分片）。
//!
//! 拆出原因：`message.rs` 已达 914 行，贴 `scripts/fn-nloc-ratchet.sh` 的
//! `MAX_FILE_LINES_CAP = 920` 上限，而构造器面还要继续补齐（见 #1235 的
//! `tool_result` / `assistant_tool_calls` / `assistant_empty`）。先把既有构造器
//! 搬到这里腾出余量，避免后续为了塞进一行空间去做无意义的压缩。
//!
//! Rust 允许多个 `impl` 块跨文件，对外路径仍是 `cm_types::Message::*`；本模块是
//! 私有 `mod`，**不需要** `pub use`——`impl` 里的项不是可再导出的名字。

use super::message::{
    CONTEXT_SUMMARY_INJECTION_CONTENT_PREFIX, CRABMATE_CONTEXT_SUMMARY_NAME,
    CRABMATE_EXECUTION_CONSTRAINT_HINT_NAME, CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME,
    CRABMATE_PLAN_REWRITE_NAME, CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME, Message, MessageContent,
};

impl Message {
    /// 分阶段规划：每步完成后的短分隔线。仅用于 UI 与同步，调用模型前须过滤。
    pub fn chat_ui_separator(short: bool) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(MessageContent::Text(
                if short { "short" } else { "long" }.to_string(),
            )),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some("crabmate_ui_sep".to_string()),
            tool_call_id: None,
        }
    }

    /// 无 `tool_calls` 的 `system` 消息（Web 首轮、CLI、TUI 空会话等共用）。
    pub fn system_only(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(MessageContent::Text(content.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    /// Act 执行约束启发式注入的临时 system（[`CRABMATE_EXECUTION_CONSTRAINT_HINT_NAME`]）；**不得**长期留在 `messages` 中。
    pub fn system_execution_constraint_hint(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(MessageContent::Text(content.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(CRABMATE_EXECUTION_CONSTRAINT_HINT_NAME.to_string()),
            tool_call_id: None,
        }
    }

    /// 无 `tool_calls` 的 `user` 消息。
    pub fn user_only(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(MessageContent::Text(content.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    /// LLM 中间段摘要（`user` + [`CRABMATE_CONTEXT_SUMMARY_NAME`]）；落盘并送模型，Web 快照过滤。
    pub fn user_context_summary_injection(summary_text: impl Into<String>) -> Self {
        let text = summary_text.into();
        Self::user_server_injection(
            CRABMATE_CONTEXT_SUMMARY_NAME,
            format!(
                "{}\n{}",
                CONTEXT_SUMMARY_INJECTION_CONTENT_PREFIX,
                text.trim()
            ),
        )
    }

    /// 首轮工作区上下文（活文档摘要 / 备忘 / 依赖摘要；`user` + [`CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME`]）；送模型，Web 快照过滤。
    pub fn user_first_turn_workspace_context(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(MessageContent::Text(content.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME.to_string()),
            tool_call_id: None,
        }
    }

    /// 规划轮 tool_calls 拒绝后的一次性重写约束（[`CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME`]）；Web 快照过滤。
    pub fn user_planner_tool_call_reject_injection(content: impl Into<String>) -> Self {
        Self::user_server_injection(CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME, content)
    }

    /// 服务端编排注入 user（`user.name` 须为 `CRABMATE_*` 注册名）。
    pub fn user_server_injection(name: &'static str, content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(MessageContent::Text(content.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(name.to_string()),
            tool_call_id: None,
        }
    }

    /// 终答规划重写 / 语义反馈 user。
    pub fn user_plan_rewrite_injection(content: impl Into<String>) -> Self {
        Self::user_server_injection(CRABMATE_PLAN_REWRITE_NAME, content)
    }

    /// 无 `tool_calls` 的 `assistant` 消息（如分阶段规划补丁合并后的 JSON 快照）。
    pub fn assistant_only(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text(content.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }
}
