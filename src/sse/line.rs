//! 对经 SSE `data:` 下发的一行字符串做分类（与 `super::protocol` 及历史兼容键名对齐）。
//!
//! Web 前端在 `frontend/src/api.ts` 的 `sendChatStream` 中做等价解析；此处为 **Rust 侧** 单一实现，供 TUI 等消费同一字节流时使用。

/// 与 `protocol` 模块对齐的 SSE 控制行；无法识别则视为模型流式正文。
#[derive(Debug)]
pub enum AgentLineKind {
    ToolRunning(bool),
    ParsingToolCalls(bool),
    WorkspaceRefresh,
    ToolCall {
        name: Option<String>,
        summary: Option<String>,
    },
    CommandApproval {
        command: String,
        args: String,
        allowlist_key: Option<String>,
    },
    ToolResult {
        name: Option<String>,
        summary: Option<String>,
        ok: Option<bool>,
        exit_code: Option<i32>,
        error_code: Option<String>,
    },
    StreamError {
        error_preview: Option<String>,
        code: Option<String>,
    },
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
            super::protocol::SsePayload::ToolCall { tool_call } => {
                let name = non_empty_string(tool_call.name);
                let summary = summarize_stream_error(&tool_call.summary);
                return AgentLineKind::ToolCall { name, summary };
            }
            super::protocol::SsePayload::CommandApproval {
                command_approval_request,
            } => {
                return AgentLineKind::CommandApproval {
                    command: command_approval_request.command,
                    args: command_approval_request.args,
                    allowlist_key: command_approval_request.allowlist_key,
                };
            }
            super::protocol::SsePayload::ToolResult { tool_result } => {
                let name = non_empty_string(tool_result.name);
                let summary = tool_result
                    .summary
                    .as_deref()
                    .and_then(summarize_stream_error);
                return AgentLineKind::ToolResult {
                    name,
                    summary,
                    ok: tool_result.ok,
                    exit_code: tool_result.exit_code,
                    error_code: tool_result.error_code,
                };
            }
            super::protocol::SsePayload::Error(body) => {
                return AgentLineKind::StreamError {
                    error_preview: summarize_stream_error(&body.error),
                    code: body.code,
                };
            }
            super::protocol::SsePayload::StagedPlanNotice { text, clear_before } => {
                return AgentLineKind::StagedPlanNotice { text, clear_before };
            }
            super::protocol::SsePayload::ChatUiSeparator { .. } => {
                // TUI 聊天区分隔线已随 `messages` 全量同步，勿再追加。
                return AgentLineKind::Ignore;
            }
            super::protocol::SsePayload::StagedPlanStarted { .. }
            | super::protocol::SsePayload::StagedPlanStepStarted { .. }
            | super::protocol::SsePayload::StagedPlanStepFinished { .. }
            | super::protocol::SsePayload::StagedPlanFinished { .. } => {
                // 结构化分步事件当前由 Web 侧优先消费；TUI 继续使用 staged_plan_notice 队列文本。
                return AgentLineKind::Ignore;
            }
            super::protocol::SsePayload::PlanRequired { .. } => {
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
    if s.starts_with("{\"tool_call\"")
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(s)
        && let Some(obj) = v.get("tool_call")
    {
        let name = obj
            .get("name")
            .and_then(|x| x.as_str())
            .map(|x| x.to_string())
            .and_then(non_empty_string);
        let summary = obj
            .get("summary")
            .and_then(|x| x.as_str())
            .and_then(summarize_stream_error);
        return AgentLineKind::ToolCall { name, summary };
    }
    if s.starts_with("{\"error\"") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
            let preview = v
                .get("error")
                .and_then(|x| x.as_str())
                .and_then(summarize_stream_error);
            let code = v
                .get("code")
                .and_then(|x| x.as_str())
                .map(|x| x.to_string());
            return AgentLineKind::StreamError {
                error_preview: preview,
                code,
            };
        }
        return AgentLineKind::StreamError {
            error_preview: None,
            code: None,
        };
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

fn summarize_stream_error(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let mut out: String = t.chars().take(80).collect();
    if t.chars().count() > 80 {
        out.push('…');
    }
    Some(out)
}

fn non_empty_string(s: String) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_protocol_error_carries_code_and_preview() {
        let line = r#"{"v":1,"error":"token expired when calling upstream","code":"AUTH"}"#;
        match classify_agent_sse_line(line) {
            AgentLineKind::StreamError {
                error_preview,
                code,
            } => {
                assert_eq!(code.as_deref(), Some("AUTH"));
                assert!(error_preview.unwrap_or_default().contains("token expired"));
            }
            other => panic!("unexpected kind: {:?}", other),
        }
    }

    #[test]
    fn parse_legacy_error_json() {
        let line = r#"{"error":"bad gateway","code":"UPSTREAM"}"#;
        match classify_agent_sse_line(line) {
            AgentLineKind::StreamError {
                error_preview,
                code,
            } => {
                assert_eq!(code.as_deref(), Some("UPSTREAM"));
                assert_eq!(error_preview.as_deref(), Some("bad gateway"));
            }
            other => panic!("unexpected kind: {:?}", other),
        }
    }

    #[test]
    fn parse_tool_result_failure_with_fields() {
        let line = r#"{"v":1,"tool_result":{"name":"run_command","summary":"执行命令 git status","output":"退出码：1","ok":false,"exit_code":1,"error_code":"command_failed","stderr":"permission denied"}}"#;
        match classify_agent_sse_line(line) {
            AgentLineKind::ToolResult {
                name,
                summary,
                ok,
                exit_code,
                error_code,
            } => {
                assert_eq!(name.as_deref(), Some("run_command"));
                assert_eq!(summary.as_deref(), Some("执行命令 git status"));
                assert_eq!(ok, Some(false));
                assert_eq!(exit_code, Some(1));
                assert_eq!(error_code.as_deref(), Some("command_failed"));
            }
            other => panic!("unexpected kind: {:?}", other),
        }
    }

    #[test]
    fn parse_tool_call_summary() {
        let line = r#"{"v":1,"tool_call":{"name":"run_command","summary":"执行命令 git status"}}"#;
        match classify_agent_sse_line(line) {
            AgentLineKind::ToolCall { name, summary } => {
                assert_eq!(name.as_deref(), Some("run_command"));
                assert_eq!(summary.as_deref(), Some("执行命令 git status"));
            }
            other => panic!("unexpected kind: {:?}", other),
        }
    }

    #[test]
    fn parse_staged_plan_structured_event_as_ignore() {
        let line = r#"{"v":1,"staged_plan_step_started":{"plan_id":"p-1","step_id":"s1","step_index":1,"total_steps":2,"description":"desc"}}"#;
        assert!(matches!(
            classify_agent_sse_line(line),
            AgentLineKind::Ignore
        ));
    }
}
