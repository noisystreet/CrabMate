//! `SsePayload` → `AgUiEvent` 转换映射。
//!
//! 覆盖所有 22 个 `SsePayload` 变体到 AG-UI 标准事件的完整映射。
//! `ToolCall` 拆分为 `ToolCallStart` + `ToolCallArgs` + `ToolCallEnd` 三事件。

use super::ag_ui_event::{AgUiErrorBody, AgUiEvent};
use super::protocol::SsePayload;

/// 将单个 `SsePayload` 转换为一个或多个 AG-UI 事件。
///
/// 大部分变体为 1:1 映射；`ToolCall` 拆分为 START/ARGS/END 三事件。
pub(crate) fn convert_sse_payload_to_ag_ui(payload: &SsePayload) -> Vec<AgUiEvent> {
    match payload {
        // ── 生命周期 ──
        SsePayload::StreamEnded { ended } => vec![AgUiEvent::RunFinished {
            thread_id: String::new(),
            run_id: ended.job_id.to_string(),
        }],

        // ── 错误 → RunError ──
        SsePayload::Error(body) => vec![AgUiEvent::RunError {
            thread_id: String::new(),
            run_id: String::new(),
            error: AgUiErrorBody {
                message: body.error.clone(),
                code: body.code.clone(),
            },
        }],

        // ── 工具调用拆为 START/ARGS/END ──
        SsePayload::ToolCall { tool_call } => {
            let tool_call_id = tool_call
                .tool_call_id
                .clone()
                .unwrap_or_else(|| format!("tc-{}", fast_random_id()));
            let parent_msg_id = format!("msg-assistant-{}", fast_random_id());
            let args = tool_call
                .arguments
                .clone()
                .or_else(|| tool_call.arguments_preview.clone())
                .unwrap_or_default();
            vec![
                AgUiEvent::ToolCallStart {
                    tool_call_id: tool_call_id.clone(),
                    name: tool_call.name.clone(),
                    parent_message_id: parent_msg_id,
                },
                AgUiEvent::ToolCallArgs {
                    tool_call_id: tool_call_id.clone(),
                    args,
                },
                AgUiEvent::ToolCallEnd { tool_call_id },
            ]
        }

        // ── 工具结果 ──
        SsePayload::ToolResult { tool_result } => {
            let tool_call_id = tool_result
                .tool_call_id
                .clone()
                .unwrap_or_else(|| format!("tc-{}", fast_random_id()));
            vec![AgUiEvent::ToolCallResult {
                tool_call_id,
                content: tool_result.output.clone(),
                metadata: Some(serde_json::json!({
                    "name": tool_result.name,
                    "ok": tool_result.ok,
                    "exitCode": tool_result.exit_code,
                    "errorCode": tool_result.error_code,
                    "failureCategory": tool_result.failure_category,
                    "summary": tool_result.summary,
                })),
            }]
        }

        // ── 工具输出片段（标记 partial）──
        SsePayload::ToolOutputChunk { tool_output_chunk } => {
            vec![AgUiEvent::ToolCallResult {
                tool_call_id: tool_output_chunk.tool_call_id.clone(),
                content: tool_output_chunk.chunk.clone(),
                metadata: Some(serde_json::json!({
                    "seq": tool_output_chunk.seq,
                    "stream": tool_output_chunk.stream,
                    "name": tool_output_chunk.name,
                    "partial": true,
                })),
            }]
        }

        // 其余均映射为 CUSTOM 事件
        _ => vec![map_payload_to_custom(payload)],
    }
}

/// 将非核心 SsePayload 映射为 `AgUiEvent::Custom`。
fn map_payload_to_custom(payload: &SsePayload) -> AgUiEvent {
    match payload {
        SsePayload::ToolRunning { tool_running } => AgUiEvent::Custom {
            custom_type: "tool_running".into(),
            data: serde_json::json!({"running": tool_running}),
        },
        SsePayload::ParsingToolCalls { parsing_tool_calls } => AgUiEvent::Custom {
            custom_type: "parsing_tool_calls".into(),
            data: serde_json::json!({"parsing": parsing_tool_calls}),
        },
        SsePayload::AssistantAnswerPhase { .. } => AgUiEvent::Custom {
            custom_type: "assistant_answer_phase".into(),
            data: serde_json::json!({"phase": "answer"}),
        },
        SsePayload::TurnSegmentStart { start } => AgUiEvent::Custom {
            custom_type: "turn_segment_start".into(),
            data: serde_json::json!({
                "segmentId": start.segment_id,
                "kind": start.kind,
                "beforeToolCallId": start.before_tool_call_id,
            }),
        },
        SsePayload::TurnSegmentEnd { end } => AgUiEvent::Custom {
            custom_type: "turn_segment_end".into(),
            data: serde_json::json!({"segmentId": end.segment_id}),
        },
        SsePayload::TurnToolPhaseEnd { .. } => AgUiEvent::Custom {
            custom_type: "turn_tool_phase_end".into(),
            data: serde_json::json!({"phase": "tool_end"}),
        },
        SsePayload::WorkspaceChanged { workspace_changed } => AgUiEvent::Custom {
            custom_type: "workspace_changed".into(),
            data: serde_json::json!({"changed": workspace_changed}),
        },
        SsePayload::CommandApproval {
            command_approval_request,
        } => AgUiEvent::Custom {
            custom_type: "command_approval".into(),
            data: serde_json::json!({
                "command": command_approval_request.command,
                "args": command_approval_request.args,
                "allowlistKey": command_approval_request.allowlist_key,
            }),
        },
        SsePayload::ClarificationQuestionnaire {
            clarification_questionnaire,
        } => AgUiEvent::Custom {
            custom_type: "clarification_questionnaire".into(),
            data: serde_json::json!({
                "questionnaireId": clarification_questionnaire.questionnaire_id,
                "intro": clarification_questionnaire.intro,
                "questions": clarification_questionnaire.questions,
            }),
        },
        SsePayload::PlanRequired { .. } => AgUiEvent::Custom {
            custom_type: "plan_required".into(),
            data: serde_json::json!({"required": true}),
        },
        SsePayload::StagedPlanNotice { text, clear_before } => AgUiEvent::Custom {
            custom_type: "staged_plan_notice".into(),
            data: serde_json::json!({"text": text, "clearBefore": clear_before}),
        },
        SsePayload::StagedPlanStarted { started } => AgUiEvent::Custom {
            custom_type: "staged_plan_started".into(),
            data: serde_json::json!({"planId": started.plan_id, "totalSteps": started.total_steps}),
        },
        SsePayload::StagedPlanStepStarted { started } => AgUiEvent::Custom {
            custom_type: "staged_plan_step_started".into(),
            data: serde_json::json!({
                "planId": started.plan_id,
                "stepId": started.step_id,
                "stepIndex": started.step_index,
                "totalSteps": started.total_steps,
                "description": started.description,
                "executorKind": started.executor_kind,
            }),
        },
        SsePayload::StagedPlanStepFinished { finished } => AgUiEvent::Custom {
            custom_type: "staged_plan_step_finished".into(),
            data: serde_json::json!({
                "planId": finished.plan_id,
                "stepId": finished.step_id,
                "stepIndex": finished.step_index,
                "totalSteps": finished.total_steps,
                "status": finished.status,
                "executorKind": finished.executor_kind,
                "verifyFailReason": finished.verify_fail_reason,
            }),
        },
        SsePayload::StagedPlanFinished { finished } => AgUiEvent::Custom {
            custom_type: "staged_plan_finished".into(),
            data: serde_json::json!({
                "planId": finished.plan_id,
                "totalSteps": finished.total_steps,
                "completedSteps": finished.completed_steps,
                "status": finished.status,
            }),
        },
        SsePayload::ChatUiSeparator { short } => AgUiEvent::Custom {
            custom_type: "chat_ui_separator".into(),
            data: serde_json::json!({"short": short}),
        },
        SsePayload::ConversationSaved { saved } => AgUiEvent::Custom {
            custom_type: "conversation_saved".into(),
            data: serde_json::json!({
                "revision": saved.revision,
                "tiktokenPromptTokens": saved.tiktoken_prompt_tokens,
            }),
        },
        SsePayload::TimelineLog { log } => AgUiEvent::Custom {
            custom_type: "timeline_log".into(),
            data: serde_json::json!({"kind": log.kind, "title": log.title, "detail": log.detail}),
        },
        SsePayload::ThinkingTrace { trace } => AgUiEvent::Custom {
            custom_type: "thinking_trace".into(),
            data: serde_json::json!({
                "op": trace.op,
                "nodeId": trace.node_id,
                "parentId": trace.parent_id,
                "title": trace.title,
                "chunk": trace.chunk,
                "contextSnapshot": trace.context_snapshot,
            }),
        },
        SsePayload::SseCapabilities { caps } => AgUiEvent::Custom {
            custom_type: "sse_capabilities".into(),
            data: serde_json::json!({
                "supportedSseV": caps.supported_sse_v,
                "resumeRingCap": caps.resume_ring_cap,
                "jobId": caps.job_id,
            }),
        },
        // 前 5 个变体已在 `convert_sse_payload_to_ag_ui` 中处理，不应到达这里
        SsePayload::StreamEnded { .. }
        | SsePayload::Error(_)
        | SsePayload::ToolCall { .. }
        | SsePayload::ToolResult { .. }
        | SsePayload::ToolOutputChunk { .. } => {
            unreachable!("handled in convert_sse_payload_to_ag_ui")
        }
    }
}

/// 快速生成短暂唯一的 ID（非加密安全，仅用于占位消息 id / tool_call_id）。
fn fast_random_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:012x}", (nanos & 0xffff_ffff_ffff))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sse::protocol::StreamEndReason;
    use crate::sse::protocol::{
        ClarificationQuestionField, ClarificationQuestionnaireBody, SseCapabilitiesBody,
        SseErrorBody, StagedPlanStartedBody, StreamEndedBody, ThinkingTraceBody, TimelineLogBody,
        ToolCallSummary, ToolOutputChunkBody, ToolResultBody,
    };

    #[test]
    fn convert_stream_ended_to_run_finished() {
        let events = convert_sse_payload_to_ag_ui(&SsePayload::StreamEnded {
            ended: StreamEndedBody {
                job_id: 42,
                reason: StreamEndReason::Completed,
                tiktoken_prompt_tokens: None,
            },
        });
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgUiEvent::RunFinished { thread_id, run_id } => {
                assert_eq!(run_id, "42");
                assert_eq!(thread_id, "");
            }
            other => panic!("expected RunFinished, got {other:?}"),
        }
    }

    #[test]
    fn convert_error_to_run_error() {
        let events = convert_sse_payload_to_ag_ui(&SsePayload::Error(SseErrorBody {
            error: "oops".into(),
            code: Some("ERR".into()),
            reason_code: None,
            turn_id: None,
            sub_phase: None,
        }));
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgUiEvent::RunError { error, .. } => {
                assert_eq!(error.message, "oops");
                assert_eq!(error.code.as_deref(), Some("ERR"));
            }
            other => panic!("expected RunError, got {other:?}"),
        }
    }

    #[test]
    fn convert_tool_call_to_three_events() {
        let events = convert_sse_payload_to_ag_ui(&SsePayload::ToolCall {
            tool_call: ToolCallSummary {
                name: "read_file".into(),
                summary: "读取文件".into(),
                goal_id: None,
                tool_call_id: Some("tc-1".into()),
                arguments_preview: Some("path=/etc/hosts".into()),
                arguments: None,
            },
        });
        assert_eq!(events.len(), 3, "ToolCall must split into 3 events");
        assert!(
            matches!(&events[0], AgUiEvent::ToolCallStart { name, .. } if name == "read_file"),
            "first event should be ToolCallStart"
        );
        assert!(
            matches!(&events[1], AgUiEvent::ToolCallArgs { tool_call_id, .. } if tool_call_id == "tc-1"),
            "second event should be ToolCallArgs"
        );
        assert!(
            matches!(&events[2], AgUiEvent::ToolCallEnd { tool_call_id } if tool_call_id == "tc-1"),
            "third event should be ToolCallEnd"
        );
    }

    #[test]
    fn convert_tool_result() {
        let events = convert_sse_payload_to_ag_ui(&SsePayload::ToolResult {
            tool_result: ToolResultBody {
                name: "run_command".into(),
                goal_id: None,
                result_version: 1,
                summary: Some("ls".into()),
                output: "file1\nfile2".into(),
                ok: Some(true),
                exit_code: Some(0),
                error_code: None,
                failure_category: None,
                retryable: None,
                tool_call_id: Some("tc-1".into()),
                execution_mode: Some("serial".into()),
                parallel_batch_id: None,
                stdout: None,
                stderr: None,
                structured_preview: None,
            },
        });
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgUiEvent::ToolCallResult {
                tool_call_id,
                content,
                metadata,
            } => {
                assert_eq!(tool_call_id, "tc-1");
                assert_eq!(content, "file1\nfile2");
                assert!(metadata.is_some());
            }
            other => panic!("expected ToolCallResult, got {other:?}"),
        }
    }

    #[test]
    fn convert_tool_output_chunk() {
        let events = convert_sse_payload_to_ag_ui(&SsePayload::ToolOutputChunk {
            tool_output_chunk: ToolOutputChunkBody {
                tool_call_id: "tc-1".into(),
                name: Some("terminal_session".into()),
                seq: 1,
                chunk: "building...".into(),
                stream: Some("stdout".into()),
            },
        });
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgUiEvent::ToolCallResult {
                content, metadata, ..
            } => {
                assert_eq!(content, "building...");
                let m = metadata.as_ref().unwrap();
                assert_eq!(m["partial"], true);
                assert_eq!(m["seq"], 1);
            }
            other => panic!("expected ToolCallResult, got {other:?}"),
        }
    }

    #[test]
    fn convert_tool_running() {
        let events = convert_sse_payload_to_ag_ui(&SsePayload::ToolRunning { tool_running: true });
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgUiEvent::Custom { custom_type, data } => {
                assert_eq!(custom_type, "tool_running");
                assert_eq!(data["running"], true);
            }
            other => panic!("expected Custom, got {other:?}"),
        }
    }

    #[test]
    fn convert_custom_event_types() {
        let test_cases: Vec<(SsePayload, &str)> = vec![
            (
                SsePayload::TimelineLog {
                    log: TimelineLogBody {
                        kind: "approval_decision".into(),
                        title: "已批准".into(),
                        detail: None,
                    },
                },
                "timeline_log",
            ),
            (
                SsePayload::StagedPlanStarted {
                    started: StagedPlanStartedBody {
                        plan_id: "p1".into(),
                        total_steps: 3,
                    },
                },
                "staged_plan_started",
            ),
            (
                SsePayload::SseCapabilities {
                    caps: SseCapabilitiesBody {
                        supported_sse_v: 2,
                        resume_ring_cap: 512,
                        job_id: 1,
                    },
                },
                "sse_capabilities",
            ),
        ];
        for (payload, expected_type) in test_cases {
            let events = convert_sse_payload_to_ag_ui(&payload);
            assert_eq!(events.len(), 1);
            match &events[0] {
                AgUiEvent::Custom { custom_type, .. } => {
                    assert_eq!(custom_type, expected_type, "for payload {payload:?}");
                }
                other => panic!("expected Custom({expected_type}), got {other:?}"),
            }
        }
    }

    #[test]
    fn convert_conversation_saved() {
        let events = convert_sse_payload_to_ag_ui(&SsePayload::ConversationSaved {
            saved: crate::sse::protocol::ConversationSavedBody {
                revision: 3,
                tiktoken_prompt_tokens: None,
            },
        });
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgUiEvent::Custom { custom_type, data } => {
                assert_eq!(custom_type, "conversation_saved");
                assert_eq!(data["revision"], 3);
            }
            other => panic!("expected Custom, got {other:?}"),
        }
    }

    #[test]
    fn convert_thinking_trace() {
        let events = convert_sse_payload_to_ag_ui(&SsePayload::ThinkingTrace {
            trace: ThinkingTraceBody {
                op: "reasoning_delta".into(),
                node_id: Some("n1".into()),
                parent_id: None,
                title: None,
                chunk: Some("thinking...".into()),
                context_snapshot: None,
            },
        });
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgUiEvent::Custom { custom_type, data } => {
                assert_eq!(custom_type, "thinking_trace");
                assert_eq!(data["op"], "reasoning_delta");
                assert_eq!(data["chunk"], "thinking...");
            }
            other => panic!("expected Custom, got {other:?}"),
        }
    }

    #[test]
    fn convert_clarification_questionnaire() {
        let events = convert_sse_payload_to_ag_ui(&SsePayload::ClarificationQuestionnaire {
            clarification_questionnaire: ClarificationQuestionnaireBody {
                questionnaire_id: "q1".into(),
                intro: "请补充信息".into(),
                questions: vec![ClarificationQuestionField {
                    id: "scope".into(),
                    label: "范围？".into(),
                    hint: Some("可选".into()),
                    required: Some(true),
                    kind: Some("text".into()),
                }],
            },
        });
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgUiEvent::Custom { custom_type, data } => {
                assert_eq!(custom_type, "clarification_questionnaire");
                assert_eq!(data["intro"], "请补充信息");
            }
            other => panic!("expected Custom, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_without_id_auto_generates() {
        let events = convert_sse_payload_to_ag_ui(&SsePayload::ToolCall {
            tool_call: ToolCallSummary {
                name: "read_file".into(),
                summary: "x".into(),
                goal_id: None,
                tool_call_id: None,
                arguments_preview: None,
                arguments: None,
            },
        });
        assert_eq!(events.len(), 3);
        match (&events[0], &events[1], &events[2]) {
            (
                AgUiEvent::ToolCallStart {
                    tool_call_id: id1, ..
                },
                AgUiEvent::ToolCallArgs {
                    tool_call_id: id2, ..
                },
                AgUiEvent::ToolCallEnd { tool_call_id: id3 },
            ) => {
                assert!(!id1.is_empty(), "auto-generated id must not be empty");
                assert_eq!(id1, id2, "id must be consistent across split events");
                assert_eq!(id1, id3, "id must be consistent across split events");
            }
            _ => panic!("unexpected event variants"),
        }
    }
}

#[cfg(test)]
mod golden_tests {
    use crate::sse::{
        ConversationSavedBody, SseCapabilitiesBody, SseEncoder, SseErrorBody, SsePayload,
        StagedPlanFinishedBody, StagedPlanStartedBody, StagedPlanStepFinishedBody,
        StagedPlanStepStartedBody, StreamEndedBody, TurnSegmentEndBody, TurnSegmentStartBody,
    };

    /// 验证 V2Encoder 输出所有 SsePayload 变体时 JSON 包含正确的 `type` 字段。
    #[test]
    fn v2_encoder_all_variants_have_type_field() {
        let encoder = crate::sse::V2Encoder;
        // 当前仅 AG-UI（v2）

        let payloads: Vec<(SsePayload, &str)> = vec![
            (
                SsePayload::AssistantAnswerPhase {
                    assistant_answer_phase: true,
                },
                "CUSTOM",
            ),
            (
                SsePayload::WorkspaceChanged {
                    workspace_changed: true,
                },
                "CUSTOM",
            ),
            (SsePayload::ToolRunning { tool_running: true }, "CUSTOM"),
            (
                SsePayload::ParsingToolCalls {
                    parsing_tool_calls: true,
                },
                "CUSTOM",
            ),
            (
                SsePayload::StagedPlanStarted {
                    started: StagedPlanStartedBody {
                        plan_id: "p".into(),
                        total_steps: 1,
                    },
                },
                "CUSTOM",
            ),
            (
                SsePayload::StagedPlanStepStarted {
                    started: StagedPlanStepStartedBody {
                        plan_id: "p".into(),
                        step_id: "s".into(),
                        step_index: 1,
                        total_steps: 1,
                        description: "d".into(),
                        executor_kind: None,
                    },
                },
                "CUSTOM",
            ),
            (
                SsePayload::StagedPlanStepFinished {
                    finished: StagedPlanStepFinishedBody {
                        plan_id: "p".into(),
                        step_id: "s".into(),
                        step_index: 1,
                        total_steps: 1,
                        status: "ok".into(),
                        executor_kind: None,
                        verify_fail_reason: None,
                    },
                },
                "CUSTOM",
            ),
            (
                SsePayload::StagedPlanFinished {
                    finished: StagedPlanFinishedBody {
                        plan_id: "p".into(),
                        total_steps: 1,
                        completed_steps: 1,
                        status: "ok".into(),
                    },
                },
                "CUSTOM",
            ),
            (
                SsePayload::TurnSegmentStart {
                    start: TurnSegmentStartBody {
                        segment_id: "s".into(),
                        kind: "k".into(),
                        before_tool_call_id: None,
                    },
                },
                "CUSTOM",
            ),
            (
                SsePayload::TurnSegmentEnd {
                    end: TurnSegmentEndBody {
                        segment_id: "s".into(),
                    },
                },
                "CUSTOM",
            ),
            (
                SsePayload::TurnToolPhaseEnd {
                    turn_tool_phase_end: true,
                },
                "CUSTOM",
            ),
            (
                SsePayload::ConversationSaved {
                    saved: ConversationSavedBody {
                        revision: 1,
                        tiktoken_prompt_tokens: None,
                    },
                },
                "CUSTOM",
            ),
            (
                SsePayload::SseCapabilities {
                    caps: SseCapabilitiesBody {
                        supported_sse_v: 2,
                        resume_ring_cap: 512,
                        job_id: 1,
                    },
                },
                "CUSTOM",
            ),
            (
                SsePayload::StreamEnded {
                    ended: StreamEndedBody {
                        job_id: 1,
                        reason: crate::StreamEndReason::Completed,
                        tiktoken_prompt_tokens: None,
                    },
                },
                "RUN_FINISHED",
            ),
            (
                SsePayload::Error(SseErrorBody {
                    error: "e".into(),
                    code: Some("ERR".into()),
                    reason_code: None,
                    sub_phase: None,
                    turn_id: None,
                }),
                "RUN_ERROR",
            ),
        ];

        for (payload, expected_type) in &payloads {
            let encoded = encoder.encode(payload);
            let v: serde_json::Value = serde_json::from_str(&encoded)
                .unwrap_or_else(|e| panic!("V2Encoder output invalid JSON: {e}\n  raw: {encoded}"));
            let actual_type = v.get("type").and_then(|t| t.as_str());
            assert_eq!(
                actual_type,
                Some(*expected_type),
                "V2Encoder type mismatch for payload\n  expected: {expected_type}\n  got: {encoded}",
            );
        }
    }
}
