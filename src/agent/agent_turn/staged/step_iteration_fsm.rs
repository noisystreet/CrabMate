//! 分阶段 **`run_staged_plan_steps_loop`** 单次迭代在 **transition 已处理之后** 的纯决策：
//! outer_loop + 验收结果如何归类、工具健康检查阶段走哪条路径；以及墙钟是否超限（与循环顶部一致）。
//! **`StagedStepRunningSub`** 与 **`docs/design/per_state_machine_consolidation.md`** 中 `StepRunning.sub` 对齐（命名略宽：`AfterOuterLoop` 含成功收尾与失败补丁）。
//! **不**运行 outer_loop / 补丁 LLM / 不发 SSE。

use crate::agent::agent_turn::errors::RunAgentTurnError;

/// 单步执行器内子阶段（对应设计稿 **`StepRunning.sub`**：`BeforeStepLlm` / `InOuterLoop` / 失败处理子集）。
/// 实现上由 **`staged/mod.rs`** 的 **`staged_step_run_outer_half`** / **`staged_step_run_after_outer_half`** 对应；本类型为**词汇表**（检索/文档对齐），生产路径不直接分支于该枚举。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedStepRunningSub {
    /// `step_started` 起至注入本步 user、设置 `turn_planner_hints.step_executor_constraint` 止（尚未 `run_agent_outer_loop`）。
    BeforeStepLlm,
    /// `run_agent_outer_loop` 与可选 acceptance 验证。
    InOuterLoop,
    /// outer 返回之后：transition、执行/验收失败补丁、取消、工具消息检查与补丁、或成功 SSE（设计稿中的 *AfterStepFailure* 为该阶段内子路径）。
    AfterOuterLoop,
}

/// `try_apply_staged_plan_control_flow_jump` 未触发时，根据 outer_loop 与验收结果划分阶段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StagedStepAfterOuterLoop {
    /// 执行与验收均成功，进入「本步 user 之后 tool 消息是否均 ok」的检查。
    ProceedToToolCheck,
    /// 执行失败或验收失败；由调用方跑补丁循环或早退。
    ExecutionOrVerifyFailed {
        outer_loop_error: Option<String>,
        verify_failure_reason: Option<String>,
    },
}

pub(crate) fn staged_step_after_outer_loop(
    run_step: &Result<(), RunAgentTurnError>,
    step_verify_failed_reason: &Option<String>,
) -> StagedStepAfterOuterLoop {
    if let Err(e) = run_step {
        return StagedStepAfterOuterLoop::ExecutionOrVerifyFailed {
            outer_loop_error: Some(e.to_string()),
            verify_failure_reason: None,
        };
    }
    if let Some(r) = step_verify_failed_reason {
        return StagedStepAfterOuterLoop::ExecutionOrVerifyFailed {
            outer_loop_error: None,
            verify_failure_reason: Some(r.clone()),
        };
    }
    StagedStepAfterOuterLoop::ProceedToToolCheck
}

/// 失败路径上补丁耗尽时构造 `StepRetryExhausted` 文案（与历史 `run_staged_plan_steps_loop` 一致）。
pub(crate) fn staged_step_failure_retry_exhausted_message(
    run_step: &Result<(), RunAgentTurnError>,
    step_verify_failed_reason: &Option<String>,
) -> String {
    if let Err(e) = run_step {
        return e.to_string();
    }
    step_verify_failed_reason
        .clone()
        .unwrap_or_else(|| "局部修复耗尽上限".to_string())
}

/// 工具消息检查阶段：是否进入「工具未全部成功」的补丁尝试循环。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedStepToolPhaseRoute {
    /// 发送本步 `ok` 并推进（含 `tools_ok==false` 且未启用 patch planner 时沿用既有语义）。
    EmitStepSuccess,
    /// `tools_ok==false` 且启用 patch planner：由调用方跑补丁循环，可能 `continue` 同一步。
    AttemptToolFailurePatches,
}

/// 单次 `run_staged_plan_steps_loop` 迭代结束方式（不含墙钟：由外层检查）。
pub(crate) enum StagedStepIterationCtl {
    /// 补丁重规划后重试当前下标（`i` 不变）。
    RetryCurrentStep { n: usize },
    /// 本步已完结（transition 或成功），调用方将 `i += 1`。
    AdvanceToNextStep { n: usize, completed_steps: usize },
    /// 本步成功后检测到取消（与历史：先发 `step_finished(cancelled)` 再 `break`）。
    CancelledAfterOuterOk,
}

pub(crate) fn staged_step_tool_phase_route(
    tools_ok: bool,
    patch_planner_enabled: bool,
) -> StagedStepToolPhaseRoute {
    if tools_ok {
        StagedStepToolPhaseRoute::EmitStepSuccess
    } else if patch_planner_enabled {
        StagedStepToolPhaseRoute::AttemptToolFailurePatches
    } else {
        StagedStepToolPhaseRoute::EmitStepSuccess
    }
}

/// 与 [`crate::agent::turn_budget::turn_wall_clock_exceeded`] 一致：`max_turn_duration_seconds == 0` 表示不限制。
pub(crate) fn staged_step_wall_clock_exceeded(
    max_turn_duration_seconds: u64,
    elapsed_secs: u64,
) -> bool {
    crate::agent::turn_budget::turn_wall_clock_exceeded(max_turn_duration_seconds, elapsed_secs)
}

pub(crate) fn staged_step_verify_fail_patch_detail(
    verify_reason: &str,
    acceptance_ref: Option<&crate::agent::plan_artifact::PlanStepAcceptance>,
) -> String {
    let reference_line = acceptance_ref
        .and_then(|a| a.compact_reference_for_planner_feedback())
        .map(|line| format!("- **参考验收（acceptance，r）**：{line}\n"))
        .unwrap_or_default();
    format!(
        "### 偏差结构化（验证失败）\n\
         {reference_line}\
         - **观测 / 偏差（step_verifier）**：{verify_reason}\n\
         若 `观测` 行以 `exit_code_mismatch:`、`stdout_missing:`、`stderr_missing:`、`combined_output_missing:`、`file_not_found:`、`json_path_mismatch:` 等键开头，请对症调整命令、工具选择或 `acceptance` 锚点。\n\
         请根据对话历史缩短或调整后续步骤，并在补丁中修复此问题。"
    )
}

/// 本分步内未全部成功的 `role: tool` 摘要，供补丁规划 **user** 的「观测 y」段落。
pub(crate) fn staged_step_tool_failure_patch_detail(
    messages: &[crate::types::Message],
    step_user_index: usize,
    acceptance_ref: Option<&crate::agent::plan_artifact::PlanStepAcceptance>,
) -> String {
    const PREVIEW_CHARS: usize = 240;
    const MAX_TOOL_LINES: usize = 6;

    let reference_line = acceptance_ref
        .and_then(|a| a.compact_reference_for_planner_feedback())
        .map(|line| format!("- **参考验收（acceptance，r）**：{line}\n"))
        .unwrap_or_default();

    if step_user_index >= messages.len() {
        return format!(
            "### 偏差结构化（工具未全部成功）\n\
             {reference_line}\
             - **观测**：`step_user_index` 越界；请直接阅读对话历史中本分步内的 `role: tool`。\n\
             {STAGED_STEP_TOOL_MSG_FAIL_DETAIL}"
        );
    }

    let mut lines: Vec<String> = Vec::new();
    let mut saw_repeat_short_circuit = false;
    let end = crate::types::staged_step_window_end_exclusive(messages, step_user_index);
    let mut i = step_user_index.saturating_add(1);
    while i < end {
        let m = &messages[i];
        if m.role == "tool" {
            let name = m.name.as_deref().unwrap_or("");
            let content = crate::types::message_content_as_str(&m.content).unwrap_or("");
            if !crate::tool_result::tool_message_content_ok_for_model(content, name) {
                let tool_error_code = tool_failure_error_code(name, content);
                saw_repeat_short_circuit |=
                    tool_error_code_is_repeat_short_circuit(tool_error_code.as_deref());
                let ec = tool_error_code
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(|s| format!("error_code={s}"))
                    .unwrap_or_else(|| "error_code=(none_or_unparsed)".to_string());
                let preview =
                    crate::redact::preview_chars(content, PREVIEW_CHARS).replace('`', "'");
                lines.push(format!("- **工具 `{name}`**：{ec}；输出摘要：{preview}"));
                if lines.len() >= MAX_TOOL_LINES {
                    lines.push(
                        "- **…**：更多失败工具已省略；请读取完整 `role: tool` 历史。".to_string(),
                    );
                    break;
                }
            }
        }
        i += 1;
    }

    let obs_block = if lines.is_empty() {
        format!(
            "### 偏差结构化（工具未全部成功）\n\
             {reference_line}\
             - **观测**：未解析到具体失败工具条目；请扫本分步内全部 `role: tool`。\n"
        )
    } else {
        format!(
            "### 偏差结构化（工具未全部成功）\n\
             {reference_line}\
             {}\n",
            lines.join("\n")
        )
    };

    format!(
        "{obs_block}\
         {}\
         若 `error_code=` 可对应 `invalid_args` / `timeout` / `not_found` 等，请在补丁中调整工具入参、白名单或前置只读步。\n\
         {STAGED_STEP_TOOL_MSG_FAIL_DETAIL}",
        repeat_short_circuit_patch_rule(saw_repeat_short_circuit)
    )
}

fn tool_failure_error_code(tool_name: &str, content: &str) -> Option<String> {
    crate::tool_result::normalize_tool_message_content(content)
        .and_then(|env| env.error_code)
        .or_else(|| crate::tool_result::parse_legacy_output(tool_name, content).error_code)
}

fn tool_error_code_is_repeat_short_circuit(error_code: Option<&str>) -> bool {
    matches!(
        error_code,
        Some("repeated_tool_failure_short_circuit" | "repeated_tool_family_failure_short_circuit")
    )
}

fn repeat_short_circuit_patch_rule(saw_repeat_short_circuit: bool) -> &'static str {
    if saw_repeat_short_circuit {
        "- **硬约束**：本步已触发重复失败短路；补丁计划不得再次生成相同 `run_command` 或同类命令。必须改为读取配置/解释失败原因/换用不同构建配置或直接向用户报告阻塞原因。\n"
    } else {
        ""
    }
}

/// 执行子循环 `Err` 时写入补丁规划 **user** 的详情（截断，避免撑爆上下文）。
pub(crate) fn staged_step_exec_fail_patch_detail(outer_loop_error: &str) -> String {
    const MAX_ERR_CHARS: usize = 1200;
    let tail = crate::redact::preview_chars(outer_loop_error, MAX_ERR_CHARS);
    format!(
        "{}\n- 执行子循环错误摘要：{}",
        STAGED_STEP_OUTER_LOOP_FAIL_DETAIL, tail
    )
}

pub(crate) const STAGED_STEP_OUTER_LOOP_FAIL_DETAIL: &str =
    "请根据对话历史缩短或调整后续步骤；若属环境/权限问题请在补丁中显式增加修复步。";

pub(crate) const STAGED_STEP_TOOL_MSG_FAIL_DETAIL: &str = "请阅读本步对应的 `role: tool` 输出（含失败原因），修订从当前步起的 `steps`（可替换、拆分或追加一步）。";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::agent_turn::errors::{AgentTurnSubPhase, RunAgentTurnError};

    #[test]
    fn after_outer_loop_err_skips_verify() {
        let err = Err(RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Executor,
            message: "x".into(),
        });
        let r = staged_step_after_outer_loop(&err, &Some("verify".into()));
        assert_eq!(
            r,
            StagedStepAfterOuterLoop::ExecutionOrVerifyFailed {
                outer_loop_error: Some("x".into()),
                verify_failure_reason: None,
            }
        );
    }

    #[test]
    fn after_outer_loop_ok_and_verify_fail() {
        let ok = Ok(());
        let r = staged_step_after_outer_loop(&ok, &Some("bad".into()));
        assert_eq!(
            r,
            StagedStepAfterOuterLoop::ExecutionOrVerifyFailed {
                outer_loop_error: None,
                verify_failure_reason: Some("bad".into()),
            }
        );
    }

    #[test]
    fn after_outer_loop_proceed() {
        let ok = Ok(());
        assert_eq!(
            staged_step_after_outer_loop(&ok, &None),
            StagedStepAfterOuterLoop::ProceedToToolCheck
        );
    }

    #[test]
    fn exhausted_message_prefers_outer_err() {
        let err = Err(RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Executor,
            message: "oe".into(),
        });
        assert_eq!(
            staged_step_failure_retry_exhausted_message(&err, &Some("v".into())),
            "oe"
        );
    }

    #[test]
    fn exhausted_message_verify_or_default() {
        let ok = Ok(());
        assert_eq!(
            staged_step_failure_retry_exhausted_message(&ok, &Some("vf".into())),
            "vf"
        );
        assert_eq!(
            staged_step_failure_retry_exhausted_message(&ok, &None),
            "局部修复耗尽上限"
        );
    }

    #[test]
    fn verify_fail_patch_detail_includes_acceptance_reference() {
        use crate::agent::plan_artifact::PlanStepAcceptance;
        let acc = PlanStepAcceptance {
            expect_exit_code: Some(0),
            expect_stdout_contains: Some("needle".into()),
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: None,
        };
        let d = staged_step_verify_fail_patch_detail(
            "exit_code_mismatch: expected 0, got 1",
            Some(&acc),
        );
        assert!(d.contains("expect_exit_code=0"));
        assert!(d.contains("expect_stdout_contains=needle"));
        assert!(d.contains("exit_code_mismatch"));
    }

    #[test]
    fn verify_fail_patch_detail_without_acceptance_reference() {
        let d = staged_step_verify_fail_patch_detail("no tool result", None);
        assert!(d.contains("no tool result"));
        assert!(!d.contains("参考验收"));
    }

    #[test]
    fn tool_fail_patch_detail_summarizes_failed_tools() {
        use crate::types::{Message, MessageContent};

        let tool_fail = Message {
            role: "tool".to_string(),
            content: Some(MessageContent::Text("退出码：1\n标准错误：\nfail\n".into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some("read_file".to_string()),
            tool_call_id: None,
        };
        let messages = vec![
            Message::user_only("step"),
            tool_fail,
            Message::user_only("next"),
        ];
        let d = staged_step_tool_failure_patch_detail(&messages, 0, None);
        assert!(d.contains("read_file"));
        assert!(d.contains("偏差结构化"));
    }

    #[test]
    fn tool_fail_patch_detail_includes_acceptance_reference() {
        use crate::agent::plan_artifact::PlanStepAcceptance;
        use crate::types::{Message, MessageContent};

        let acc = PlanStepAcceptance {
            expect_exit_code: Some(0),
            expect_stdout_contains: Some("ok".into()),
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: None,
        };
        let tool_fail = Message {
            role: "tool".to_string(),
            content: Some(MessageContent::Text("退出码：1\n".into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some("run_command".to_string()),
            tool_call_id: None,
        };
        let messages = vec![Message::user_only("step"), tool_fail];
        let d = staged_step_tool_failure_patch_detail(&messages, 0, Some(&acc));
        assert!(d.contains("expect_exit_code=0"));
        assert!(d.contains("run_command"));
    }

    #[test]
    fn tool_fail_patch_detail_forbids_repeat_short_circuit_retry() {
        use crate::types::{Message, MessageContent};

        let raw =
            "错误：检测到同命令重复失败，已短路本次调用（error=run_command_failed）。请切换策略。";
        let parsed = crate::tool_result::parse_legacy_output("run_command", raw);
        let envelope = crate::tool_result::encode_tool_message_envelope_v1(
            "run_command",
            "make".into(),
            &parsed,
            raw,
            None,
        );
        let tool_fail = Message {
            role: "tool".to_string(),
            content: Some(MessageContent::Text(envelope)),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some("run_command".to_string()),
            tool_call_id: None,
        };
        let messages = vec![Message::user_only("step"), tool_fail];
        let d = staged_step_tool_failure_patch_detail(&messages, 0, None);

        assert!(d.contains("硬约束"));
        assert!(d.contains("error_code=repeated_tool_failure_short_circuit"));
        assert!(d.contains("不得再次生成相同 `run_command`"));
    }

    #[test]
    fn exec_fail_patch_detail_includes_error_tail() {
        let d = staged_step_exec_fail_patch_detail("context canceled");
        assert!(d.contains(STAGED_STEP_OUTER_LOOP_FAIL_DETAIL));
        assert!(d.contains("context canceled"));
    }

    #[test]
    fn tool_phase_routes() {
        assert_eq!(
            staged_step_tool_phase_route(true, false),
            StagedStepToolPhaseRoute::EmitStepSuccess
        );
        assert_eq!(
            staged_step_tool_phase_route(true, true),
            StagedStepToolPhaseRoute::EmitStepSuccess
        );
        assert_eq!(
            staged_step_tool_phase_route(false, false),
            StagedStepToolPhaseRoute::EmitStepSuccess
        );
        assert_eq!(
            staged_step_tool_phase_route(false, true),
            StagedStepToolPhaseRoute::AttemptToolFailurePatches
        );
    }

    #[test]
    fn wall_clock_exceeded_matches_loop() {
        assert!(!staged_step_wall_clock_exceeded(0, 999));
        assert!(!staged_step_wall_clock_exceeded(10, 10));
        assert!(staged_step_wall_clock_exceeded(10, 11));
    }
}
