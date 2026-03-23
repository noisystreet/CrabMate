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
    /// 分阶段规划：TUI 用于状态栏与「队列」页；Web 可忽略（`frontend/src/api.ts` 吞掉不当下文）。
    StagedPlanNotice {
        /// 可多行 `\n` 分隔。
        #[serde(rename = "staged_plan_notice")]
        text: String,
        /// 为 true 时客户端清空本轮规划日志再追加 `text` 各行。
        #[serde(default, rename = "staged_plan_notice_clear")]
        clear_before: bool,
    },
    /// 分阶段规划：结构化「计划已生成」事件（供 Web/TUI 精准展示进度，不依赖文本解析）。
    StagedPlanStarted {
        #[serde(rename = "staged_plan_started")]
        started: StagedPlanStartedBody,
    },
    /// 分阶段规划：单步开始事件。
    StagedPlanStepStarted {
        #[serde(rename = "staged_plan_step_started")]
        started: StagedPlanStepStartedBody,
    },
    /// 分阶段规划：单步结束事件。
    StagedPlanStepFinished {
        #[serde(rename = "staged_plan_step_finished")]
        finished: StagedPlanStepFinishedBody,
    },
    /// 分阶段规划：整轮计划结束事件。
    StagedPlanFinished {
        #[serde(rename = "staged_plan_finished")]
        finished: StagedPlanFinishedBody,
    },
    /// 分阶段规划：每步结束短分隔线。TUI 随 `messages` 同步已有行；Web 用本事件追加。（`false` 保留兼容，客户端可忽略。）
    ChatUiSeparator {
        /// `true` 为短分隔线。
        #[serde(rename = "chat_ui_separator")]
        short: bool,
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
    /// 与 `summarize_tool_call` 同源；与 `output` 同帧下发，供 Web 在工具结束后再展示「先摘要后输出」。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StagedPlanStartedBody {
    pub plan_id: String,
    pub total_steps: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StagedPlanStepStartedBody {
    pub plan_id: String,
    pub step_id: String,
    /// 从 1 开始的人类可读序号。
    pub step_index: usize,
    pub total_steps: usize,
    pub description: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StagedPlanStepFinishedBody {
    pub plan_id: String,
    pub step_id: String,
    /// 从 1 开始的人类可读序号。
    pub step_index: usize,
    pub total_steps: usize,
    /// `ok` / `cancelled` / `failed`
    pub status: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StagedPlanFinishedBody {
    pub plan_id: String,
    pub total_steps: usize,
    pub completed_steps: usize,
    /// `ok` / `cancelled` / `failed`
    pub status: String,
}

/// 序列化为单行 JSON，供 `Event::data(...)` 使用。
pub fn encode_message(payload: SsePayload) -> String {
    serde_json::to_string(&SseMessage {
        v: SSE_PROTOCOL_VERSION,
        payload,
    })
    .unwrap_or_else(|e| {
        log::error!(
            target: "crabmate",
            "sse_protocol encode failed error={}",
            e
        );
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
                summary: Some("执行命令 ls".into()),
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
                assert_eq!(tool_result.summary.as_deref(), Some("执行命令 ls"));
                assert_eq!(tool_result.ok, Some(false));
                assert_eq!(tool_result.exit_code, Some(1));
                assert_eq!(tool_result.error_code.as_deref(), Some("command_failed"));
                assert_eq!(tool_result.stderr.as_deref(), Some("permission denied"));
            }
            _ => panic!("expected tool_result payload"),
        }
    }

    #[test]
    fn roundtrip_staged_plan_notice() {
        let s = encode_message(SsePayload::StagedPlanNotice {
            text: "【规划】共 2 步\n  1. [a] x".into(),
            clear_before: true,
        });
        let m: SseMessage = serde_json::from_str(&s).unwrap();
        match m.payload {
            SsePayload::StagedPlanNotice { text, clear_before } => {
                assert!(clear_before);
                assert_eq!(text, "【规划】共 2 步\n  1. [a] x");
            }
            _ => panic!("expected staged_plan_notice payload"),
        }
    }

    #[test]
    fn roundtrip_chat_ui_separator() {
        let s = encode_message(SsePayload::ChatUiSeparator { short: true });
        assert!(s.contains("\"chat_ui_separator\":true"));
        let m: SseMessage = serde_json::from_str(&s).unwrap();
        assert!(matches!(
            m.payload,
            SsePayload::ChatUiSeparator { short: true }
        ));
        let s2 = encode_message(SsePayload::ChatUiSeparator { short: false });
        let m2: SseMessage = serde_json::from_str(&s2).unwrap();
        assert!(matches!(
            m2.payload,
            SsePayload::ChatUiSeparator { short: false }
        ));
    }

    #[test]
    fn roundtrip_staged_plan_structured_events() {
        let started = encode_message(SsePayload::StagedPlanStarted {
            started: StagedPlanStartedBody {
                plan_id: "plan-1".into(),
                total_steps: 3,
            },
        });
        let msg_started: SseMessage = serde_json::from_str(&started).unwrap();
        match msg_started.payload {
            SsePayload::StagedPlanStarted { started } => {
                assert_eq!(started.plan_id, "plan-1");
                assert_eq!(started.total_steps, 3);
            }
            _ => panic!("expected staged_plan_started payload"),
        }

        let step_started = encode_message(SsePayload::StagedPlanStepStarted {
            started: StagedPlanStepStartedBody {
                plan_id: "plan-1".into(),
                step_id: "collect-context".into(),
                step_index: 1,
                total_steps: 3,
                description: "收集上下文".into(),
            },
        });
        let msg_step_started: SseMessage = serde_json::from_str(&step_started).unwrap();
        match msg_step_started.payload {
            SsePayload::StagedPlanStepStarted { started } => {
                assert_eq!(started.step_id, "collect-context");
                assert_eq!(started.step_index, 1);
                assert_eq!(started.total_steps, 3);
            }
            _ => panic!("expected staged_plan_step_started payload"),
        }

        let step_finished = encode_message(SsePayload::StagedPlanStepFinished {
            finished: StagedPlanStepFinishedBody {
                plan_id: "plan-1".into(),
                step_id: "collect-context".into(),
                step_index: 1,
                total_steps: 3,
                status: "failed".into(),
            },
        });
        let msg_step_finished: SseMessage = serde_json::from_str(&step_finished).unwrap();
        match msg_step_finished.payload {
            SsePayload::StagedPlanStepFinished { finished } => {
                assert_eq!(finished.status, "failed");
                assert_eq!(finished.step_index, 1);
            }
            _ => panic!("expected staged_plan_step_finished payload"),
        }

        let finished = encode_message(SsePayload::StagedPlanFinished {
            finished: StagedPlanFinishedBody {
                plan_id: "plan-1".into(),
                total_steps: 3,
                completed_steps: 3,
                status: "ok".into(),
            },
        });
        let msg_finished: SseMessage = serde_json::from_str(&finished).unwrap();
        match msg_finished.payload {
            SsePayload::StagedPlanFinished { finished } => {
                assert_eq!(finished.completed_steps, 3);
                assert_eq!(finished.status, "ok");
            }
            _ => panic!("expected staged_plan_finished payload"),
        }
    }
}
