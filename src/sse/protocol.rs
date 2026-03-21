//! 经 SSE `data:` 行发往浏览器 / TUI 的**控制类** JSON 协议（与 `llm::api::stream_chat` 下发的纯文本 delta 区分）。
//!
//! 统一带版本字段 `v`，键名与现有前端 `frontend/src/api.ts` 兼容；新增事件通过新键扩展。

/// 当前协议版本；演进时递增并在前端做分支解析（若需）。
pub const SSE_PROTOCOL_VERSION: u8 = 1;

fn default_sse_v() -> u8 {
    1
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SseMessage {
    #[serde(default = "default_sse_v")]
    pub v: u8,
    #[serde(flatten)]
    pub payload: SsePayload,
}

/// 控制面负载：`untagged` 按字段形状区分，顺序从更特异的结构到更通用的结构。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum SsePayload {
    /// 流式对话失败（`chat_stream_handler` 等）
    Error(SseErrorBody),
    /// 工作流 / TUI 命令审批
    CommandApproval {
        command_approval_request: CommandApprovalBody,
    },
    /// 工具调用摘要（执行前）
    ToolCall {
        tool_call: ToolCallSummary,
    },
    /// 工具完整输出
    ToolResult {
        tool_result: ToolResultBody,
    },
    WorkspaceChanged {
        workspace_changed: bool,
    },
    ToolRunning {
        tool_running: bool,
    },
    /// 模型正在流式输出 tool_calls（选工具 / 解析参数），尚未进入本地工具执行
    ParsingToolCalls {
        parsing_tool_calls: bool,
    },
    /// 预留：例如 PER 要求前端提示「须补充结构化规划」
    PlanRequired {
        plan_required: bool,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SseErrorBody {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommandApprovalBody {
    pub command: String,
    pub args: String,
    /// TUI 永久允许时写入磁盘白名单的键；缺省则使用 `command` 小写（与 `run_command` 行为一致）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist_key: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCallSummary {
    pub name: String,
    pub summary: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolResultBody {
    pub name: String,
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
}

/// 序列化为单行 JSON，供 `Event::data(...)` 使用。
pub fn encode_message(payload: SsePayload) -> String {
    serde_json::to_string(&SseMessage {
        v: SSE_PROTOCOL_VERSION,
        payload,
    })
    .unwrap_or_else(|e| {
        tracing::error!(%e, "sse_protocol encode failed");
        r#"{"v":1,"error":"内部协议序列化失败","code":"SSE_ENCODE"}"#.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_parsing_tool_calls() {
        let s = encode_message(SsePayload::ParsingToolCalls {
            parsing_tool_calls: true,
        });
        let m: SseMessage = serde_json::from_str(&s).unwrap();
        assert!(matches!(
            m.payload,
            SsePayload::ParsingToolCalls {
                parsing_tool_calls: true
            }
        ));
    }

    #[test]
    fn roundtrip_tool_running() {
        let s = encode_message(SsePayload::ToolRunning { tool_running: true });
        let m: SseMessage = serde_json::from_str(&s).unwrap();
        assert_eq!(m.v, SSE_PROTOCOL_VERSION);
        assert!(matches!(
            m.payload,
            SsePayload::ToolRunning { tool_running: true }
        ));
    }

    #[test]
    fn deserialize_legacy_no_v_field() {
        let m: SseMessage = serde_json::from_str(r#"{"tool_running":false}"#).unwrap();
        assert_eq!(m.v, 1);
        assert!(matches!(
            m.payload,
            SsePayload::ToolRunning {
                tool_running: false
            }
        ));
    }

    #[test]
    fn error_with_code() {
        let s = encode_message(SsePayload::Error(SseErrorBody {
            error: "x".into(),
            code: Some("E".into()),
        }));
        assert!(s.contains("\"v\":1"));
        assert!(s.contains("\"code\":\"E\""));
    }

    #[test]
    fn tool_result_with_structured_fields() {
        let s = encode_message(SsePayload::ToolResult {
            tool_result: ToolResultBody {
                name: "run_command".into(),
                output: "退出码：1".into(),
                ok: Some(false),
                exit_code: Some(1),
                error_code: Some("command_failed".into()),
                stdout: Some(String::new()),
                stderr: Some("permission denied".into()),
            },
        });
        let m: SseMessage = serde_json::from_str(&s).unwrap();
        match m.payload {
            SsePayload::ToolResult { tool_result } => {
                assert_eq!(tool_result.name, "run_command");
                assert_eq!(tool_result.ok, Some(false));
                assert_eq!(tool_result.exit_code, Some(1));
                assert_eq!(tool_result.error_code.as_deref(), Some("command_failed"));
                assert_eq!(tool_result.stderr.as_deref(), Some("permission denied"));
            }
            _ => panic!("expected tool_result payload"),
        }
    }
}
