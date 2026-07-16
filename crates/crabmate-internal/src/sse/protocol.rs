//! 经 SSE `data:` 行发往浏览器等的**控制类** JSON 协议（与 `llm::api::stream_chat` 下发的纯文本 delta 区分）。
//!
//! 统一带版本字段 `v`，键名与现有前端 **`frontend/src/api/`**（`chat_stream` 等）兼容；新增事件通过新键扩展。
//!
//! **完整契约**（版本、`error`/`code` 与 `tool_result.error_code` 枚举、双端对齐清单）见仓库 **`docs/SSE协议.md`**（与 `frontend/src/sse_dispatch/dispatch.rs` 对齐）。

pub use crabmate_sse_protocol::{SSE_PROTOCOL_VERSION, StreamEndReason};

/// 服务端为每条 `/chat/stream` SSE 事件分配的 **`id:`**（`Last-Event-ID`）环形缓冲容量（仅内存；进程重启后不可恢复）。
pub const SSE_RESUME_RING_CAP: usize = 512;

fn default_sse_v() -> u8 {
    SSE_PROTOCOL_VERSION
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
    /// 工作流 / 命令审批（Web 等）
    CommandApproval {
        command_approval_request: CommandApprovalBody,
    },
    /// Web 澄清问卷：模型经工具 `present_clarification_questionnaire` 成功后由 `execute_tools` 补发；用户下一条 `POST /chat*` 带 `clarify_questionnaire_answers`。
    ClarificationQuestionnaire {
        #[serde(rename = "clarification_questionnaire")]
        clarification_questionnaire: ClarificationQuestionnaireBody,
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
    /// 工具执行中的输出片段（如 PTY / 长命令）；**不**进入模型上下文；最终以 [`ToolResult`] 收束。
    ToolOutputChunk {
        #[serde(rename = "tool_output_chunk")]
        tool_output_chunk: ToolOutputChunkBody,
    },
    /// 模型正在流式输出 tool_calls（选工具 / 解析参数），尚未进入本地工具执行
    ParsingToolCalls {
        parsing_tool_calls: bool,
    },
    /// 后续 SSE 纯文本增量为助手 **终答** `content`（此前为思维链 `reasoning_*`）；无思维链时也会在**首段**正文前下发，供 Web 分色。
    AssistantAnswerPhase {
        #[serde(rename = "assistant_answer_phase")]
        assistant_answer_phase: bool,
    },
    /// 回合段开始：锚定「某 `tool_call_id` 之前」的旁注（晚到 delta 仍挂此锚点）。
    TurnSegmentStart {
        #[serde(rename = "turn_segment_start")]
        start: TurnSegmentStartBody,
    },
    /// 回合段结束：关闭 `turn_segment_start` 所开段。
    TurnSegmentEnd {
        #[serde(rename = "turn_segment_end")]
        end: TurnSegmentEndBody,
    },
    /// 工具批结束：后续正文增量为 post-tool 终答（与 `assistant_answer_phase` 配合）。
    TurnToolPhaseEnd {
        #[serde(rename = "turn_tool_phase_end")]
        turn_tool_phase_end: bool,
    },
    /// 预留：例如 PER 要求前端提示「须补充结构化规划」
    PlanRequired {
        plan_required: bool,
    },
    /// 分阶段规划：前端可忽略（**`frontend/src/api/chat_stream/`** 等路径吞掉不当下文）。
    StagedPlanNotice {
        /// 可多行 `\n` 分隔。
        #[serde(rename = "staged_plan_notice")]
        text: String,
        /// 为 true 时客户端清空本轮规划日志再追加 `text` 各行。
        #[serde(default, rename = "staged_plan_notice_clear")]
        clear_before: bool,
    },
    /// 分阶段规划：结构化「计划已生成」事件（供 Web 精准展示进度，不依赖文本解析）。
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
    /// 分阶段规划：每步结束短分隔线。Web 用本事件追加。（`false` 保留兼容，客户端可忽略。）
    ChatUiSeparator {
        /// `true` 为短分隔线。
        #[serde(rename = "chat_ui_separator")]
        short: bool,
    },
    /// 本会话已成功写入存储后的 revision（供 Web 分叉 `POST /chat/branch` 与冲突检测）。
    ConversationSaved {
        #[serde(rename = "conversation_saved")]
        saved: ConversationSavedBody,
    },
    /// 时间线旁注（审批结果等）；Web 可展示，**不**进入模型上下文。
    TimelineLog {
        #[serde(rename = "timeline_log")]
        log: TimelineLogBody,
    },
    /// 思维过程调试：结构化 trace（默认下发；`CM_THINKING_TRACE_ENABLED=0` 关闭；不进模型上下文）。
    ThinkingTrace {
        #[serde(rename = "thinking_trace")]
        trace: ThinkingTraceBody,
    },
    /// 首帧能力协商：`supported_sse_v` 与 Rust `SSE_PROTOCOL_VERSION` 一致；`resume_ring_cap` 为环形缓冲条数。
    SseCapabilities {
        #[serde(rename = "sse_capabilities")]
        caps: SseCapabilitiesBody,
    },
    /// 流正常结束（任务完成或已从 hub 注销）；客户端可停止重连。
    StreamEnded {
        #[serde(rename = "stream_ended")]
        ended: StreamEndedBody,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SseErrorBody {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// 与 `code` 配合的**细分子码**（如 `code=plan_rewrite_exhausted` 时的失败类别），供客户端分支处理；旧客户端忽略即可。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    /// Web 流任务 id（与响应头 **`x-stream-job-id`** / `sse_capabilities.job_id` 一致）；非 Web 路径可省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<u64>,
    /// 失败时所处的编排子阶段：`planner` \| `executor` \| `reflect`（与 `agent_turn` PER 命名对齐）；旧客户端忽略即可。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_phase: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommandApprovalBody {
    pub command: String,
    pub args: String,
    /// 永久允许时写入进程内白名单的键；缺省则使用 `command` 小写（与 `run_command` 行为一致）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist_key: Option<String>,
}

/// 澄清问卷单题字段（与 `present_clarification_questionnaire` 工具参数对齐）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClarificationQuestionField {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// `text` 或 `choice`（预留；Web 当前均以文本提交）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClarificationQuestionnaireBody {
    pub questionnaire_id: String,
    pub intro: String,
    pub questions: Vec<ClarificationQuestionField>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCallSummary {
    pub name: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
    /// 与本轮 `tool_calls[].id` / `tool_result.tool_call_id` 对齐，供前端将结果写回正确占位气泡。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// 与 `redact::tool_arguments_preview_for_sse` 一致：单行截断的 `function.arguments` 预览；缺省省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments_preview: Option<String>,
    /// 配置 `sse_tool_call_include_arguments` 为真时下发；经 `redact::tool_arguments_redacted_for_sse` 脱敏后截断。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

/// [`SsePayload::ToolOutputChunk`] 内层体：与 `tool_call` / `tool_result` 共用 **`tool_call_id`**。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolOutputChunkBody {
    pub tool_call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub seq: u64,
    #[serde(default)]
    pub chunk: String,
    /// `stdout` / `stderr` / `combined`（PTY 常为合并流，省略或 `combined`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
}

fn default_tool_result_payload_version() -> u32 {
    crate::tool_result::CRABMATE_TOOL_ENVELOPE_VERSION_V1
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolResultBody {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
    /// 与 `crabmate_tool.v` 对齐的**工具结果载荷版本**（区别于顶层 `SseMessage.v` / `SSE_PROTOCOL_VERSION`）。
    #[serde(default = "default_tool_result_payload_version")]
    pub result_version: u32,
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
    /// 与 `tool_error::ToolFailureCategory::as_str` / `crabmate_tool.failure_category` 一致；失败时由 `error_code` 推导。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_category: Option<String>,
    /// 与 `crabmate_tool.retryable` 一致：启发式，非保证。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// `serial` 或 `parallel_readonly_batch`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_batch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    /// 可选：与工具 `output` 首行 **`crabmate_tool_output`** JSON 同源的小型结构化预览（**不含**文件正文），供 Web/集成方解析。
    /// 当前由 **`read_file`** / **`read_dir`** / **`list_tree`** 等只读文件工具填充；其它工具省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_preview: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TurnSegmentStartBody {
    pub segment_id: String,
    /// `commentary`（工具前旁注）或 `answer`（终答段）。
    pub kind: String,
    /// 若非空：本段展示在该 `tool_call_id` **之前**。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_tool_call_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TurnSegmentEndBody {
    pub segment_id: String,
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
    /// 分阶段 `steps[].executor_kind`（蛇形）；无则省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor_kind: Option<String>,
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
    /// 与 `staged_plan_step_started` 对齐；无则省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor_kind: Option<String>,
    /// 验证失败原因（当 `status` 为 `failed` 且失败源于步级验收时填充）。
    /// 格式示例：`exit_code_mismatch: expected 0, got 1`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_fail_reason: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StagedPlanFinishedBody {
    pub plan_id: String,
    pub total_steps: usize,
    pub completed_steps: usize,
    /// `ok` / `cancelled` / `failed`
    pub status: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConversationSavedBody {
    pub revision: u64,
    /// 落盘后会话 prompt token 粗估（tiktoken-rs；与 `GET /conversation/messages` 同规则）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiktoken_prompt_tokens: Option<crabmate_types::TiktokenPromptTokensSnapshot>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SseCapabilitiesBody {
    pub supported_sse_v: u8,
    pub resume_ring_cap: usize,
    /// 本流在队列与 hub 中的 `job_id`；断线重连时填入 `stream_resume.job_id`。
    pub job_id: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StreamEndedBody {
    pub job_id: u64,
    /// `completed` | `cancelled` | `conflict` | `fallback` | `no_output` | `gone`
    pub reason: StreamEndReason,
    /// 回合结束时的 prompt token 粗估（先于或并行于 `conversation_saved`；便于底栏即时更新）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiktoken_prompt_tokens: Option<crabmate_types::TiktokenPromptTokensSnapshot>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TimelineLogBody {
    /// 如 `approval_decision`、`approval_request`（与前端展示分类一致即可）。
    pub kind: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// `thinking_trace` 负载：`op` 区分语义；`chunk` 为推理流增量片段；`context_snapshot` 为工具前后上下文摘要（非全文）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThinkingTraceBody {
    /// `reasoning_delta` | `answer_phase` | `tool_call` | `tool_done`
    pub op: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_snapshot: Option<String>,
}

/// 序列化为单行 JSON，供 `Event::data(...)` 使用。
/// 委托到当前默认编码器（`V1Encoder`）。
pub fn encode_message(payload: SsePayload) -> String {
    let encoder = super::encoder::default_encoder();
    encoder.encode(&payload)
}

/// v1 编码器内部实现（`V1Encoder::encode` 调用此函数）。
pub(crate) fn encode_message_v1(payload: &SsePayload) -> String {
    serde_json::to_string(&SseMessage {
        v: SSE_PROTOCOL_VERSION,
        payload: payload.clone(),
    })
    .unwrap_or_else(|e| {
        log::error!(
            target: "crabmate",
            "sse_protocol encode failed error={}",
            e
        );
        format!(
            r#"{{"v":{},"error":"内部协议序列化失败","code":"SSE_ENCODE"}}"#,
            SSE_PROTOCOL_VERSION
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crabmate_sse_protocol::StreamEndReason;
    use proptest::prelude::*;

    #[test]
    fn roundtrip_clarification_questionnaire() {
        let s = encode_message_v1(&SsePayload::ClarificationQuestionnaire {
            clarification_questionnaire: ClarificationQuestionnaireBody {
                questionnaire_id: "q1".into(),
                intro: "请补充".into(),
                questions: vec![ClarificationQuestionField {
                    id: "scope".into(),
                    label: "范围？".into(),
                    hint: Some("可选".into()),
                    required: Some(true),
                    kind: Some("text".into()),
                }],
            },
        });
        let m: SseMessage = serde_json::from_str(&s).unwrap();
        assert!(matches!(
            m.payload,
            SsePayload::ClarificationQuestionnaire { .. }
        ));
    }

    #[test]
    fn roundtrip_parsing_tool_calls() {
        let s = encode_message_v1(&SsePayload::ParsingToolCalls {
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
    fn roundtrip_assistant_answer_phase() {
        let s = encode_message_v1(&SsePayload::AssistantAnswerPhase {
            assistant_answer_phase: true,
        });
        assert!(s.contains("\"assistant_answer_phase\":true"));
        let m: SseMessage = serde_json::from_str(&s).unwrap();
        assert!(matches!(
            m.payload,
            SsePayload::AssistantAnswerPhase {
                assistant_answer_phase: true
            }
        ));
    }

    #[test]
    fn roundtrip_tool_running() {
        let s = encode_message_v1(&SsePayload::ToolRunning { tool_running: true });
        let m: SseMessage = serde_json::from_str(&s).unwrap();
        assert_eq!(m.v, SSE_PROTOCOL_VERSION);
        assert!(matches!(
            m.payload,
            SsePayload::ToolRunning { tool_running: true }
        ));
    }

    #[test]
    fn roundtrip_tool_output_chunk() {
        let s = encode_message_v1(&SsePayload::ToolOutputChunk {
            tool_output_chunk: ToolOutputChunkBody {
                tool_call_id: "tc1".into(),
                name: Some("terminal_session".into()),
                seq: 3,
                chunk: "hello\n".into(),
                stream: Some("combined".into()),
            },
        });
        assert!(s.contains("\"tool_output_chunk\""));
        let m: SseMessage = serde_json::from_str(&s).unwrap();
        match m.payload {
            SsePayload::ToolOutputChunk { tool_output_chunk } => {
                assert_eq!(tool_output_chunk.tool_call_id, "tc1");
                assert_eq!(tool_output_chunk.seq, 3);
                assert_eq!(tool_output_chunk.chunk, "hello\n");
                assert_eq!(tool_output_chunk.stream.as_deref(), Some("combined"));
            }
            _ => panic!("expected tool_output_chunk payload"),
        }
    }

    #[test]
    fn deserialize_legacy_no_v_field() {
        let m: SseMessage = serde_json::from_str(r#"{"tool_running":false}"#).unwrap();
        assert_eq!(m.v, SSE_PROTOCOL_VERSION);
        assert!(matches!(
            m.payload,
            SsePayload::ToolRunning {
                tool_running: false
            }
        ));
    }

    #[test]
    fn error_with_code() {
        let s = encode_message_v1(&SsePayload::Error(SseErrorBody {
            error: "x".into(),
            code: Some("E".into()),
            reason_code: None,
            turn_id: None,
            sub_phase: None,
        }));
        assert!(s.contains(&format!("\"v\":{}", SSE_PROTOCOL_VERSION)));
        assert!(s.contains("\"code\":\"E\""));
    }

    #[test]
    fn error_with_reason_code() {
        let s = encode_message_v1(&SsePayload::Error(SseErrorBody {
            error: "x".into(),
            code: Some("plan_rewrite_exhausted".into()),
            reason_code: Some("plan_missing".into()),
            turn_id: None,
            sub_phase: Some("reflect".into()),
        }));
        assert!(s.contains("\"reason_code\":\"plan_missing\""));
    }

    #[test]
    fn tool_result_with_structured_fields() {
        let s = encode_message_v1(&SsePayload::ToolResult {
            tool_result: ToolResultBody {
                name: "run_command".into(),
                goal_id: None,
                result_version: 1,
                summary: Some("ls".into()),
                output: "退出码：1".into(),
                ok: Some(false),
                exit_code: Some(1),
                error_code: Some("command_failed".into()),
                failure_category: Some("external".into()),
                retryable: Some(false),
                tool_call_id: Some("tc1".into()),
                execution_mode: Some("serial".into()),
                parallel_batch_id: None,
                stdout: Some(String::new()),
                stderr: Some("permission denied".into()),
                structured_preview: None,
            },
        });
        let m: SseMessage = serde_json::from_str(&s).unwrap();
        match m.payload {
            SsePayload::ToolResult { tool_result } => {
                assert_eq!(tool_result.name, "run_command");
                assert_eq!(tool_result.summary.as_deref(), Some("ls"));
                assert_eq!(tool_result.ok, Some(false));
                assert_eq!(tool_result.exit_code, Some(1));
                assert_eq!(tool_result.error_code.as_deref(), Some("command_failed"));
                assert_eq!(tool_result.failure_category.as_deref(), Some("external"));
                assert_eq!(tool_result.retryable, Some(false));
                assert_eq!(tool_result.tool_call_id.as_deref(), Some("tc1"));
                assert_eq!(tool_result.execution_mode.as_deref(), Some("serial"));
                assert_eq!(tool_result.stderr.as_deref(), Some("permission denied"));
                assert!(tool_result.structured_preview.is_none());
            }
            _ => panic!("expected tool_result payload"),
        }
    }

    #[test]
    fn roundtrip_staged_plan_notice() {
        let s = encode_message_v1(&SsePayload::StagedPlanNotice {
            text: "**规划** · 共 2 步\n  1. [a] x".into(),
            clear_before: true,
        });
        let m: SseMessage = serde_json::from_str(&s).unwrap();
        match m.payload {
            SsePayload::StagedPlanNotice { text, clear_before } => {
                assert!(clear_before);
                assert_eq!(text, "**规划** · 共 2 步\n  1. [a] x");
            }
            _ => panic!("expected staged_plan_notice payload"),
        }
    }

    #[test]
    fn roundtrip_chat_ui_separator() {
        let s = encode_message_v1(&SsePayload::ChatUiSeparator { short: true });
        assert!(s.contains("\"chat_ui_separator\":true"));
        let m: SseMessage = serde_json::from_str(&s).unwrap();
        assert!(matches!(
            m.payload,
            SsePayload::ChatUiSeparator { short: true }
        ));
        let s2 = encode_message_v1(&SsePayload::ChatUiSeparator { short: false });
        let m2: SseMessage = serde_json::from_str(&s2).unwrap();
        assert!(matches!(
            m2.payload,
            SsePayload::ChatUiSeparator { short: false }
        ));
    }

    #[test]
    fn roundtrip_staged_plan_structured_events() {
        let started = encode_message_v1(&SsePayload::StagedPlanStarted {
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

        let step_started = encode_message_v1(&SsePayload::StagedPlanStepStarted {
            started: StagedPlanStepStartedBody {
                plan_id: "plan-1".into(),
                step_id: "collect-context".into(),
                step_index: 1,
                total_steps: 3,
                description: "收集上下文".into(),
                executor_kind: Some("review_readonly".into()),
            },
        });
        let msg_step_started: SseMessage = serde_json::from_str(&step_started).unwrap();
        match msg_step_started.payload {
            SsePayload::StagedPlanStepStarted { started } => {
                assert_eq!(started.step_id, "collect-context");
                assert_eq!(started.step_index, 1);
                assert_eq!(started.total_steps, 3);
                assert_eq!(started.executor_kind.as_deref(), Some("review_readonly"));
            }
            _ => panic!("expected staged_plan_step_started payload"),
        }

        let step_finished = encode_message_v1(&SsePayload::StagedPlanStepFinished {
            finished: StagedPlanStepFinishedBody {
                plan_id: "plan-1".into(),
                step_id: "collect-context".into(),
                step_index: 1,
                total_steps: 3,
                status: "failed".into(),
                executor_kind: Some("review_readonly".into()),
                verify_fail_reason: None,
            },
        });
        let msg_step_finished: SseMessage = serde_json::from_str(&step_finished).unwrap();
        match msg_step_finished.payload {
            SsePayload::StagedPlanStepFinished { finished } => {
                assert_eq!(finished.status, "failed");
                assert_eq!(finished.step_index, 1);
                assert_eq!(finished.executor_kind.as_deref(), Some("review_readonly"));
            }
            _ => panic!("expected staged_plan_step_finished payload"),
        }

        let finished = encode_message_v1(&SsePayload::StagedPlanFinished {
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

    fn arb_short_text() -> impl Strategy<Value = String> {
        // Keep payloads compact so CI stays fast and outputs readable.
        "[a-zA-Z0-9_\\- ]{0,32}".prop_map(|s| s.trim().to_string())
    }

    proptest! {
        #[test]
        fn prop_tool_running_roundtrip_and_version(tool_running in any::<bool>()) {
            let encoded = encode_message_v1(&SsePayload::ToolRunning { tool_running });
            let parsed: SseMessage = serde_json::from_str(&encoded).unwrap();
            prop_assert_eq!(parsed.v, SSE_PROTOCOL_VERSION);
            match parsed.payload {
                SsePayload::ToolRunning { tool_running: got } => prop_assert_eq!(got, tool_running),
                other => prop_assert!(false, "unexpected payload: {:?}", other),
            }
        }

        #[test]
        fn prop_error_payload_roundtrip(
            error in arb_short_text(),
            code in proptest::option::of(arb_short_text()),
            reason_code in proptest::option::of(arb_short_text()),
            turn_id in proptest::option::of(any::<u64>()),
            sub_phase in proptest::option::of(arb_short_text()),
        ) {
            let payload = SsePayload::Error(SseErrorBody {
                error,
                code,
                reason_code,
                turn_id,
                sub_phase,
            });
            let encoded = encode_message_v1(&payload);
            let parsed: SseMessage = serde_json::from_str(&encoded).unwrap();
            prop_assert_eq!(parsed.v, SSE_PROTOCOL_VERSION);
            match (payload, parsed.payload) {
                (SsePayload::Error(expect), SsePayload::Error(got)) => {
                    prop_assert_eq!(got.error, expect.error);
                    prop_assert_eq!(got.code, expect.code);
                    prop_assert_eq!(got.reason_code, expect.reason_code);
                    prop_assert_eq!(got.turn_id, expect.turn_id);
                    prop_assert_eq!(got.sub_phase, expect.sub_phase);
                }
                (_, other) => prop_assert!(false, "unexpected payload: {:?}", other),
            }
        }

        #[test]
        fn prop_stream_ended_reason_is_parsable(job_id in any::<u64>(), reason_idx in 0usize..6usize) {
            let reason = match reason_idx {
                0 => StreamEndReason::Completed,
                1 => StreamEndReason::Cancelled,
                2 => StreamEndReason::Conflict,
                3 => StreamEndReason::Fallback,
                4 => StreamEndReason::NoOutput,
                _ => StreamEndReason::Gone,
            };
            let encoded = encode_message_v1(&SsePayload::StreamEnded {
                ended: StreamEndedBody {
                    job_id,
                    reason,
                    tiktoken_prompt_tokens: None,
                },
            });
            let parsed: SseMessage = serde_json::from_str(&encoded).unwrap();
            match parsed.payload {
                SsePayload::StreamEnded { ended } => {
                    prop_assert_eq!(ended.job_id, job_id);
                    prop_assert_eq!(ended.reason, reason);
                }
                other => prop_assert!(false, "unexpected payload: {:?}", other),
            }
        }
    }
}
