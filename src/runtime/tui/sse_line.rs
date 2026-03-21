//! SSE 控制行分类（与 `crate::sse_protocol` 对齐）。

/// 与 `sse_protocol` 对齐的 SSE 控制行；无法识别则视为模型流式正文。
#[derive(Debug)]
pub(super) enum AgentLineKind {
    ToolRunning(bool),
    ParsingToolCalls(bool),
    WorkspaceRefresh,
    CommandApproval {
        command: String,
        args: String,
        allowlist_key: Option<String>,
    },
    StreamError,
    /// 已识别为协议行但无需刷新 UI（如 workspace_changed:false）
    Ignore,
    Plain,
}

pub(super) fn classify_agent_sse_line(s: &str) -> AgentLineKind {
    if let Ok(msg) = serde_json::from_str::<crate::sse_protocol::SseMessage>(s) {
        match msg.payload {
            crate::sse_protocol::SsePayload::ToolRunning { tool_running } => {
                return AgentLineKind::ToolRunning(tool_running);
            }
            crate::sse_protocol::SsePayload::ParsingToolCalls { parsing_tool_calls } => {
                return AgentLineKind::ParsingToolCalls(parsing_tool_calls);
            }
            crate::sse_protocol::SsePayload::WorkspaceChanged {
                workspace_changed: true,
            } => return AgentLineKind::WorkspaceRefresh,
            crate::sse_protocol::SsePayload::WorkspaceChanged {
                workspace_changed: false,
            } => return AgentLineKind::Ignore,
            crate::sse_protocol::SsePayload::CommandApproval {
                command_approval_request,
            } => {
                return AgentLineKind::CommandApproval {
                    command: command_approval_request.command,
                    args: command_approval_request.args,
                    allowlist_key: command_approval_request.allowlist_key,
                };
            }
            crate::sse_protocol::SsePayload::Error(_) => return AgentLineKind::StreamError,
            crate::sse_protocol::SsePayload::ToolCall { .. }
            | crate::sse_protocol::SsePayload::ToolResult { .. }
            | crate::sse_protocol::SsePayload::PlanRequired { .. } => {
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
