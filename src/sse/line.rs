//! 对经 SSE `data:` 下发的一行字符串做分类（与 `super::protocol` 及历史兼容键名对齐）。
//!
//! Web 前端在 `frontend/src/api.ts` 的 `sendChatStream` 中做等价解析；此处为 **Rust 侧** 单一实现，供 TUI 等消费同一字节流时使用。

/// 与 `protocol` 模块对齐的 SSE 控制行；无法识别则视为模型流式正文。
#[derive(Debug)]
pub enum AgentLineKind {
    ToolRunning(bool),
    ParsingToolCalls(bool),
    WorkspaceRefresh,
    CommandApproval {
        command: String,
        args: String,
        allowlist_key: Option<String>,
    },
    StreamError,
    /// 分阶段规划摘要（仅 TUI 队列页/状态栏使用；Web 忽略）
    StagedPlanNotice {
        text: String,
        clear_before: bool,
    },
    /// 已识别为协议行但无需刷新 UI（如 workspace_changed:false）
    Ignore,
    Plain,
}

pub fn classify_agent_sse_line(s: &str) -> AgentLineKind {
    if let Ok(msg) = serde_json::from_str::<super::protocol::SseMessage>(s) {
        match msg.payload {
            super::protocol::SsePayload::ToolRunning { tool_running } => {
                return AgentLineKind::ToolRunning(tool_running);
            }
            super::protocol::SsePayload::ParsingToolCalls { parsing_tool_calls } => {
                return AgentLineKind::ParsingToolCalls(parsing_tool_calls);
            }
            super::protocol::SsePayload::WorkspaceChanged {
                workspace_changed: true,
            } => return AgentLineKind::WorkspaceRefresh,
            super::protocol::SsePayload::WorkspaceChanged {
                workspace_changed: false,
            } => return AgentLineKind::Ignore,
            super::protocol::SsePayload::CommandApproval {
                command_approval_request,
            } => {
                return AgentLineKind::CommandApproval {
                    command: command_approval_request.command,
                    args: command_approval_request.args,
                    allowlist_key: command_approval_request.allowlist_key,
                };
            }
            super::protocol::SsePayload::Error(_) => return AgentLineKind::StreamError,
            super::protocol::SsePayload::StagedPlanNotice { text, clear_before } => {
                return AgentLineKind::StagedPlanNotice { text, clear_before };
            }
            super::protocol::SsePayload::ToolCall { .. }
            | super::protocol::SsePayload::ToolResult { .. }
            | super::protocol::SsePayload::PlanRequired { .. } => {
                return AgentLineKind::Ignore;
            }
        }
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(s)
        && let Some(b) = v.get("parsing_tool_calls").and_then(|x| x.as_bool())
    {
        return AgentLineKind::ParsingToolCalls(b);
    }
    if s == r#"{"tool_running":true}"# {
        return AgentLineKind::ToolRunning(true);
    }
    if s == r#"{"tool_running":false}"# {
        return AgentLineKind::ToolRunning(false);
    }
    if s == r#"{"workspace_changed":true}"# {
        return AgentLineKind::WorkspaceRefresh;
    }
    if s.starts_with("{\"error\"") {
        return AgentLineKind::StreamError;
    }
    if s.starts_with("{\"command_approval_request\"")
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(s)
        && let Some(obj) = v.get("command_approval_request")
    {
        return AgentLineKind::CommandApproval {
            command: obj
                .get("command")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            args: obj
                .get("args")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            allowlist_key: obj
                .get("allowlist_key")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
        };
    }
    AgentLineKind::Plain
}
