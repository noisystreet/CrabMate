//! S2-2：SDK 面 `TurnEvent` 的有损投影。
//!
//! 口径见 `docs/design/sdk_facade.md` §5.1.1 的 ADR，这里只复述会直接影响本文件的四条：
//!
//! 1. **真源是线上 AG-UI 帧**。文本增量不在 `SsePayload` 里（它没有 `*Delta` 变体），
//!    而在 `AgUiEvent::TextMessageContent` 上；`SsePayload` 只是 `Custom` 载荷。
//! 2. **`ToolResult` 必须带 `partial`**。`tool_output_chunk`（长任务流式片段）与
//!    `tool_result`（最终结果）在线上**都是** `TOOL_CALL_RESULT`，只靠 `metadata.partial`
//!    区分。缺了它，消费方会把每段片段当成最终结果——语义错误，不是信息缺失。
//! 3. **不投影用三态表达**：`Event` / `Dropped`（映射表里明确丢弃）/ `Unknown`（映射表漏了）。
//!    合成一个 `None` 或留一个 `Other(Value)` 兜底，都会让「新增事件必须有人做决策」
//!    的保护失效，而那正是 S2-2 要消除的漂移。
//! 4. `AgUiEvent` 只有 `Serialize`、没有 `Deserialize`，所以这里只能读 `serde_json::Value`。
//!    编译期穷尽性拿不到，改由两张常量表 + 遍历测试保证（见本文件末尾的 `tests`）。
//!
//! 当前是 `pub(crate)`：本片的目标是把口径落成**可测代码**；承诺面升级
//! （`crabmate::TurnEvent` 的公开形状与 semver 约束）属于 S6，届时再决定是否改名/收窄。

use serde_json::Value;

/// SDK 面回合事件（设计 §5.1 的五个变体）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SdkTurnEvent {
    /// `TEXT_MESSAGE_CONTENT` —— **唯一**的文本增量源。
    TextDelta { delta: String },
    /// `TOOL_CALL_START` —— 卡片标题所需字段在 START 帧已齐。
    ToolCall {
        tool_call_id: String,
        name: String,
        summary: String,
    },
    /// `TOOL_CALL_RESULT`。
    ///
    /// `partial == true` 表示这是 `tool_output_chunk` 的流式片段（见模块文档第 2 条），
    /// **不可**当作最终结果使用。
    ToolResult {
        tool_call_id: String,
        content: String,
        partial: bool,
    },
    /// `CUSTOM` + `customType == "command_approval"` —— 唯一需要人工介入的回合内事件。
    Approval {
        command: String,
        args: String,
        permission_level: Option<String>,
    },
    /// `RUN_FINISHED` / `RUN_ERROR` —— 错误也走终止，不另起 `Error` 变体，
    /// 免得消费方要在两处匹配「回合结束」。
    Finished { status: SdkTurnStatus },
}

/// 回合终止状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SdkTurnStatus {
    Ok,
    Failed {
        message: String,
        code: Option<String>,
    },
}

/// 一帧的去向。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Projection {
    /// 投影成 SDK 面事件。
    Event(SdkTurnEvent),
    /// 映射表里**明确判定丢弃**——这是决策，不是缺陷。
    Dropped,
    /// 映射表**没覆盖**的 `type` / `customType`——这是缺陷，测试会红。
    Unknown,
}

/// AG-UI 帧 `type` 全集，与 `AgUiEvent` 的 16 个变体一一对应。
///
/// 这是一张**测试清单**：生产路径只消费 [`project_frame`]，由本表驱动穷尽性测试，
/// 所以生产代码不引用它，`dead_code` 是有意的（#1231 建议的 item 级 allow）。
#[allow(dead_code)]
pub(crate) const AG_UI_KNOWN_TYPES: &[&str] = &[
    "RUN_STARTED",
    "RUN_FINISHED",
    "RUN_ERROR",
    "TEXT_MESSAGE_START",
    "TEXT_MESSAGE_CONTENT",
    "TEXT_MESSAGE_END",
    "TOOL_CALL_START",
    "TOOL_CALL_ARGS",
    "TOOL_CALL_END",
    "TOOL_CALL_RESULT",
    "REASONING_MESSAGE_START",
    "REASONING_MESSAGE_CONTENT",
    "REASONING_MESSAGE_END",
    "STATE_SNAPSHOT",
    "STATE_DELTA",
    "CUSTOM",
];

/// `customType` 全集：17 个来自 `SsePayload` 的 `Custom` 分支，加 `orchestration_route`。
///
/// 注意它**不是** `SsePayload` 的 22 个变体——另 5 个走标准 AG-UI 事件
/// （`StreamEnded` / `Error` / `ToolCall` / `ToolResult` / `ToolOutputChunk`）。
///
/// 与 [`AG_UI_KNOWN_TYPES`] 同理：这是驱动穷尽性测试的清单，生产路径不引用。
#[allow(dead_code)]
pub(crate) const CUSTOM_KNOWN_TYPES: &[&str] = &[
    "assistant_answer_phase",
    "chat_ui_separator",
    "clarification_questionnaire",
    "command_approval",
    "conversation_saved",
    "orchestration_route",
    "parsing_tool_calls",
    "plan_required",
    "sse_capabilities",
    "stream_draining",
    "thinking_trace",
    "timeline_log",
    "tool_job_finished",
    "tool_running",
    "turn_segment_end",
    "turn_segment_start",
    "turn_tool_phase_end",
    "workspace_changed",
];

/// 把一帧线上 JSON 投影成 [`Projection`]。
pub(crate) fn project_frame(frame: &Value) -> Projection {
    let Some(type_str) = frame.get("type").and_then(Value::as_str) else {
        return Projection::Unknown;
    };
    match type_str {
        "RUN_FINISHED" => Projection::Event(SdkTurnEvent::Finished {
            status: SdkTurnStatus::Ok,
        }),
        "RUN_ERROR" => Projection::Event(SdkTurnEvent::Finished {
            status: SdkTurnStatus::Failed {
                message: nested_str(frame.get("error"), &["message"]).unwrap_or_default(),
                code: nested_str(frame.get("error"), &["code"]),
            },
        }),
        "TEXT_MESSAGE_CONTENT" => Projection::Event(SdkTurnEvent::TextDelta {
            delta: nested_str(Some(frame), &["delta"]).unwrap_or_default(),
        }),
        "TOOL_CALL_START" => Projection::Event(SdkTurnEvent::ToolCall {
            tool_call_id: nested_str(Some(frame), &["toolCallId"]).unwrap_or_default(),
            name: nested_str(Some(frame), &["name"]).unwrap_or_default(),
            summary: nested_str(Some(frame), &["summary"]).unwrap_or_default(),
        }),
        "TOOL_CALL_RESULT" => Projection::Event(SdkTurnEvent::ToolResult {
            tool_call_id: nested_str(Some(frame), &["toolCallId"]).unwrap_or_default(),
            content: nested_str(Some(frame), &["content"]).unwrap_or_default(),
            // 缺省即「最终结果」：`tool_output_chunk` 一定会带 `partial: true`。
            partial: nested_bool(frame.get("metadata"), &["partial"]).unwrap_or(false),
        }),
        "CUSTOM" => project_custom(frame),
        // 表 A 明确丢弃的项（生命周期 / 增量参数 / 思维链 / 状态同步）。
        "RUN_STARTED"
        | "TEXT_MESSAGE_START"
        | "TEXT_MESSAGE_END"
        | "TOOL_CALL_ARGS"
        | "TOOL_CALL_END"
        | "REASONING_MESSAGE_START"
        | "REASONING_MESSAGE_CONTENT"
        | "REASONING_MESSAGE_END"
        | "STATE_SNAPSHOT"
        | "STATE_DELTA" => Projection::Dropped,
        _ => Projection::Unknown,
    }
}

/// `CUSTOM` 按 `customType` 分流（映射表 B）。
fn project_custom(frame: &Value) -> Projection {
    let Some(custom_type) = frame.get("customType").and_then(Value::as_str) else {
        return Projection::Unknown;
    };
    match custom_type {
        "command_approval" => {
            let data = frame.get("data");
            Projection::Event(SdkTurnEvent::Approval {
                command: nested_str(data, &["command"]).unwrap_or_default(),
                args: nested_str(data, &["args"]).unwrap_or_default(),
                permission_level: nested_str(data, &["permission_level"]),
            })
        }
        // 表 B 明确丢弃的 17 项：UI 状态位、渲染面、持久化/能力协商、编排内部路由，
        // 以及两个已知缺口（`clarification_questionnaire` 交互面、
        // `thinking_trace` 思维链）——缺口记在 §5.1.1，不在这里扩面。
        "tool_job_finished"
        | "tool_running"
        | "parsing_tool_calls"
        | "assistant_answer_phase"
        | "turn_segment_start"
        | "turn_segment_end"
        | "turn_tool_phase_end"
        | "workspace_changed"
        | "plan_required"
        | "chat_ui_separator"
        | "conversation_saved"
        | "stream_draining"
        | "timeline_log"
        | "thinking_trace"
        | "sse_capabilities"
        | "clarification_questionnaire"
        | "orchestration_route" => Projection::Dropped,
        _ => Projection::Unknown,
    }
}

/// 沿 `path` 逐层取值并转成 owned `String`；缺键 / 类型不符一律 `None`。
fn nested_str(value: Option<&Value>, path: &[&str]) -> Option<String> {
    let mut cur = value?;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_str().map(str::to_owned)
}

/// 同 [`nested_str`]，但取 `bool`。
fn nested_bool(value: Option<&Value>, path: &[&str]) -> Option<bool> {
    let mut cur = value?;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_bool()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 表 A 的每一项都必须被识别（`Event` 或 `Dropped`），不得落进 `Unknown`。
    #[test]
    fn every_known_ag_ui_type_is_recognised() {
        for type_str in AG_UI_KNOWN_TYPES {
            // `CUSTOM` 是载荷容器：裸帧缺 `customType` 属于**畸形帧**（另有断言钉住它是
            // `Unknown`），所以要带一个已知 `customType` 才能证明「这一族被处理了」。
            let frame = if *type_str == "CUSTOM" {
                json!({ "type": type_str, "customType": "command_approval", "data": {} })
            } else {
                json!({ "type": type_str })
            };
            assert_ne!(
                project_frame(&frame),
                Projection::Unknown,
                "AG-UI type `{type_str}` 未在 project_frame 中处理"
            );
        }
    }

    /// 表 B 的每一项同理。
    #[test]
    fn every_known_custom_type_is_recognised() {
        for custom_type in CUSTOM_KNOWN_TYPES {
            let frame = json!({ "type": "CUSTOM", "customType": custom_type, "data": {} });
            assert_ne!(
                project_frame(&frame),
                Projection::Unknown,
                "customType `{custom_type}` 未在 project_custom 中处理"
            );
        }
    }

    /// 表长与各自的变体数一致：新增变体却忘了更新表 → 红。
    #[test]
    fn known_type_tables_match_variant_counts() {
        assert_eq!(AG_UI_KNOWN_TYPES.len(), 16, "AgUiEvent 变体数应为 16");
        assert_eq!(CUSTOM_KNOWN_TYPES.len(), 18, "customType 全集应为 18");
    }

    /// 两张表互不相交：AG-UI 帧 type 与 customType 是两个命名空间。
    #[test]
    fn known_type_tables_are_disjoint() {
        for custom_type in CUSTOM_KNOWN_TYPES {
            assert!(
                !AG_UI_KNOWN_TYPES.contains(custom_type),
                "`{custom_type}` 同时出现在两张表里"
            );
        }
    }

    /// 兜底不是静默的：未知字面量必须落进 `Unknown`。
    #[test]
    fn unknown_type_and_custom_type_report_unknown() {
        assert_eq!(
            project_frame(&json!({ "type": "SOMETHING_NEW" })),
            Projection::Unknown
        );
        assert_eq!(
            project_frame(&json!({ "type": "CUSTOM", "customType": "something_new" })),
            Projection::Unknown
        );
        // 连 `type` 都没有的帧同样算漏网，而不是被当成空事件。
        assert_eq!(project_frame(&json!({})), Projection::Unknown);
        // 畸形帧：`CUSTOM` 却缺 `customType`。它不是「已知丢弃项」，而是无法解释。
        assert_eq!(
            project_frame(&json!({ "type": "CUSTOM" })),
            Projection::Unknown
        );
    }

    #[test]
    fn text_delta_is_the_only_text_source() {
        assert_eq!(
            project_frame(&json!({ "type": "TEXT_MESSAGE_CONTENT", "delta": "你好" })),
            Projection::Event(SdkTurnEvent::TextDelta {
                delta: "你好".to_string()
            })
        );
        // 生命周期帧不产生文本，必须被明确丢弃而不是当成空 delta。
        assert_eq!(
            project_frame(&json!({ "type": "TEXT_MESSAGE_START", "messageId": "m1" })),
            Projection::Dropped
        );
    }

    /// 事实四的钉子：片段与最终结果同走 `TOOL_CALL_RESULT`，靠 `metadata.partial` 区分。
    #[test]
    fn tool_result_distinguishes_stream_chunk_from_final() {
        let chunk = json!({
            "type": "TOOL_CALL_RESULT",
            "toolCallId": "tc-1",
            "content": "line 1\n",
            "metadata": { "partial": true, "seq": 1, "stream": "stdout" }
        });
        assert_eq!(
            project_frame(&chunk),
            Projection::Event(SdkTurnEvent::ToolResult {
                tool_call_id: "tc-1".to_string(),
                content: "line 1\n".to_string(),
                partial: true,
            })
        );

        let final_result = json!({
            "type": "TOOL_CALL_RESULT",
            "toolCallId": "tc-1",
            "content": "done"
        });
        assert_eq!(
            project_frame(&final_result),
            Projection::Event(SdkTurnEvent::ToolResult {
                tool_call_id: "tc-1".to_string(),
                content: "done".to_string(),
                partial: false,
            })
        );
    }

    #[test]
    fn tool_call_start_carries_card_title_fields() {
        assert_eq!(
            project_frame(&json!({
                "type": "TOOL_CALL_START",
                "toolCallId": "tc-9",
                "name": "read_file",
                "summary": "读取 src/lib.rs"
            })),
            Projection::Event(SdkTurnEvent::ToolCall {
                tool_call_id: "tc-9".to_string(),
                name: "read_file".to_string(),
                summary: "读取 src/lib.rs".to_string(),
            })
        );
    }

    /// 终止只有一个入口：正常与错误都走 `Finished`。
    #[test]
    fn both_finish_and_error_project_to_finished() {
        assert_eq!(
            project_frame(&json!({ "type": "RUN_FINISHED", "threadId": "t", "runId": "r" })),
            Projection::Event(SdkTurnEvent::Finished {
                status: SdkTurnStatus::Ok
            })
        );
        assert_eq!(
            project_frame(&json!({
                "type": "RUN_ERROR",
                "threadId": "t",
                "runId": "r",
                "error": { "message": "boom", "code": "ERR" }
            })),
            Projection::Event(SdkTurnEvent::Finished {
                status: SdkTurnStatus::Failed {
                    message: "boom".to_string(),
                    code: Some("ERR".to_string()),
                }
            })
        );
    }

    #[test]
    fn command_approval_projects_with_permission_level() {
        assert_eq!(
            project_frame(&json!({
                "type": "CUSTOM",
                "customType": "command_approval",
                "data": {
                    "command": "curl",
                    "args": "https://example.com",
                    "permission_level": "workspace"
                }
            })),
            Projection::Event(SdkTurnEvent::Approval {
                command: "curl".to_string(),
                args: "https://example.com".to_string(),
                permission_level: Some("workspace".to_string()),
            })
        );

        // 旧路径不带等级时省略该键：投影为 `None`，不编造默认值。
        assert_eq!(
            project_frame(&json!({
                "type": "CUSTOM",
                "customType": "command_approval",
                "data": { "command": "curl", "args": "" }
            })),
            Projection::Event(SdkTurnEvent::Approval {
                command: "curl".to_string(),
                args: String::new(),
                permission_level: None,
            })
        );
    }

    /// 已知缺口与 UI 状态位都必须是 `Dropped`（决策），不能是 `Unknown`（缺陷）。
    #[test]
    fn known_dropped_custom_types_are_decisions_not_gaps() {
        for custom_type in [
            "clarification_questionnaire",
            "thinking_trace",
            "turn_segment_start",
            "tool_running",
            "orchestration_route",
        ] {
            assert_eq!(
                project_frame(&json!({ "type": "CUSTOM", "customType": custom_type })),
                Projection::Dropped,
                "`{custom_type}` 应被明确丢弃"
            );
        }
    }
}
