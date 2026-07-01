//! 回合级「目标完成 / 早停 / 冗余工具抑制 / 终答纠偏 / 步后抑规划」共用判定。

use crate::agent::plan_artifact::{PlanStepAcceptance, PlanStepV1};
use crate::types::{
    Message, ToolCall, last_real_user_message_index, last_staged_step_injection_index,
    message_content_as_str,
};

use super::completion_suppression::{
    plan_steps_are_redundant_after_completion, plan_steps_require_formal_execution,
    tool_calls_are_redundant_when_goal_satisfied,
};
use super::task_level_evidence::{
    GoalCompletionEvidenceCheck, check_active_user_goal_completion_evidence,
    generic_task_intent_implies_build_or_test,
};

/// 最多注入次数，避免与外层迭代上限死磕。
pub(crate) const OUTER_LOOP_MISSING_FINAL_ANSWER_FEEDBACK_MAX: u32 = 2;

/// 低于该字符数（Unicode scalar）视为「无可见终答」。
pub(crate) const OUTER_LOOP_MISSING_FINAL_ANSWER_MIN_CHARS: usize = 24;

/// 当前活跃用户目标的任务级完成证据（启发式）。
pub(crate) fn turn_goal_completion_evidence(messages: &[Message]) -> GoalCompletionEvidenceCheck {
    check_active_user_goal_completion_evidence(messages)
}

/// 任务级证据已 Satisfied 时是否允许**提前停轮**（规划步滚动视界与子 Agent 外循环共用）。
///
/// 构建/测试类任务须走完规划步（含 `test_runner` / 有效 `acceptance`），不得仅凭启发式早停。
pub(crate) fn turn_early_stop_allowed(messages: &[Message]) -> bool {
    if !matches!(
        turn_goal_completion_evidence(messages),
        GoalCompletionEvidenceCheck::Satisfied
    ) {
        return false;
    }
    let Some(task) = crate::agent::plan_optimizer::staged_plan_trigger_user_content(messages)
    else {
        return false;
    };
    !generic_task_intent_implies_build_or_test(task)
}

/// 与 [`turn_early_stop_allowed`] 同义；保留旧名供逐步迁移引用。
pub(crate) fn task_level_satisfied_allows_early_stop(messages: &[Message]) -> bool {
    turn_early_stop_allowed(messages)
}

/// 活跃目标已有完成证据且允许早停时，是否应静默丢弃本轮探针类 / 重复 `run_command` 工具调用。
pub(crate) fn turn_redundant_tools_after_completion_allowed(
    tool_calls: &[ToolCall],
    messages: &[Message],
) -> bool {
    if !tool_calls_are_redundant_when_goal_satisfied(tool_calls, messages) {
        return false;
    }
    turn_early_stop_allowed(messages)
}

/// 步后重规划：目标已 Satisfied 且新 `steps` 仅为探针/总结时是否应抑制下一轮无工具规划。
pub(crate) fn turn_suppress_completed_replanning(
    messages: &[Message],
    entered_from_step_execution_round: bool,
    steps: &[PlanStepV1],
) -> bool {
    if !entered_from_step_execution_round || steps.is_empty() {
        return false;
    }
    if plan_steps_require_formal_execution(steps) {
        return false;
    }
    if !turn_early_stop_allowed(messages) {
        return false;
    }
    plan_steps_are_redundant_after_completion(steps)
}

/// 滚动视界：步执行轮结束后是否可因「目标已完成」提前结束（不再进入下一轮无工具规划）。
///
/// - 只读类：[`turn_early_stop_allowed`]
/// - 构建/测试类：须本步 [`PlanStepAcceptance`] 在步窗口内验收 **Pass**（见 hints 中上一完成步的 effective acceptance）
pub(crate) fn turn_staged_rolling_horizon_early_stop_allowed(
    messages: &[Message],
    last_completed_step_effective_acceptance: Option<&PlanStepAcceptance>,
    workspace_root: &std::path::Path,
) -> bool {
    if turn_early_stop_allowed(messages) {
        return true;
    }
    let Some(acceptance) = last_completed_step_effective_acceptance else {
        return false;
    };
    let Some(step_idx) = last_staged_step_injection_index(messages) else {
        return false;
    };
    crate::agent::step_verifier::verify_step_execution(
        acceptance,
        messages,
        step_idx,
        workspace_root,
    )
    .is_pass()
}

fn outer_loop_window_has_any_successful_tool(messages: &[Message]) -> bool {
    let Some(user_idx) = last_real_user_message_index(messages, false) else {
        return false;
    };
    messages[user_idx.saturating_add(1)..].iter().any(|m| {
        if m.role != "tool" {
            return false;
        }
        let Some(raw) = message_content_as_str(&m.content) else {
            return false;
        };
        if let Some(env) = crate::tool_result::normalize_tool_message_content(raw) {
            return env.ok || env.exit_code == Some(0);
        }
        let lower = raw.to_lowercase();
        lower.contains("退出码：0") || lower.contains("exit code: 0")
    })
}

pub(crate) fn outer_loop_assistant_lacks_visible_final_answer(msg: &Message) -> bool {
    if msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
        return false;
    }
    let text = message_content_as_str(&msg.content).unwrap_or("").trim();
    text.chars().count() < OUTER_LOOP_MISSING_FINAL_ANSWER_MIN_CHARS
}

pub(crate) fn outer_loop_missing_final_answer_feedback_body() -> String {
    format!(
        "{prefix}本轮工具已执行并产生结果，但助手终答为空或过短。请基于**当前对话与工具输出**，用自然语言向用户给出完整终答：\
         说明已完成什么、关键结果/路径/命令输出摘要，以及若仍有未完成项请明确列出。**禁止**再发起无必要的 tool_calls。",
        prefix = crabmate_display_rules::OUTER_LOOP_BUILD_IDLE_ORCHESTRATION_PREFIX
    )
}

/// 若应注入纠偏 user 并继续外循环，返回 `Some(feedback)`。
pub(crate) fn outer_loop_missing_final_answer_feedback_if_needed(
    messages: &[Message],
    assistant: &Message,
    feedback_injected_count: u32,
) -> Option<String> {
    if feedback_injected_count >= OUTER_LOOP_MISSING_FINAL_ANSWER_FEEDBACK_MAX {
        return None;
    }
    if !outer_loop_assistant_lacks_visible_final_answer(assistant) {
        return None;
    }
    if !outer_loop_window_has_any_successful_tool(messages) {
        return None;
    }
    Some(outer_loop_missing_final_answer_feedback_body())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan_artifact::PlanStepV1;
    use crate::types::Message;

    fn msg(role: &str, text: &str) -> Message {
        Message {
            role: role.to_string(),
            content: Some(text.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    fn tool_env(name: &str, summary: &str, output: &str) -> Message {
        let parsed = crate::tool_result::parse_legacy_output(name, output);
        msg(
            "tool",
            &crate::tool_result::encode_tool_message_envelope_v1(
                name,
                summary.to_string(),
                &parsed,
                output,
                None,
            ),
        )
    }

    fn step(id: &str, kind: Option<&str>, description: &str) -> PlanStepV1 {
        PlanStepV1 {
            id: id.to_string(),
            description: description.to_string(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: kind.map(str::to_string),
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        }
    }

    #[test]
    fn build_task_heuristic_satisfied_does_not_allow_staged_early_exit() {
        let messages = vec![
            msg("user", "编译 hpcg"),
            tool_env(
                "run_command",
                "make -C hpcg",
                "命令：make -C hpcg\n退出码：0\n标准输出：\nBuilt",
            ),
            msg("assistant", "HPCG 编译完成成功。"),
        ];
        assert!(!turn_early_stop_allowed(&messages));
    }

    #[test]
    fn build_task_heuristic_satisfied_blocks_outer_loop_style_early_exit() {
        let messages = vec![
            msg("user", "cmake 构建 hello"),
            tool_env(
                "run_command",
                "cmake --build build",
                "命令：cmake --build build\n退出码：0\n标准输出：\n[100%] Built target hello",
            ),
            msg("assistant", "构建已成功完成。"),
        ];
        assert!(!turn_early_stop_allowed(&messages));
    }

    #[test]
    fn readonly_task_allows_staged_early_exit_when_satisfied() {
        let messages = vec![
            msg("user", "分析当前目录"),
            tool_env("list_tree", "list tree", "list tree: ."),
            msg(
                "assistant",
                "当前目录包含三个压缩包，分析结果如下，总结完成。",
            ),
        ];
        assert!(turn_early_stop_allowed(&messages));
    }

    #[test]
    fn build_task_does_not_suppress_replanning_without_early_stop_gate() {
        let messages = vec![
            msg("user", "编译 hpcg"),
            tool_env(
                "run_command",
                "make",
                "命令：make\n退出码：0\n标准输出：\nBuilt",
            ),
            msg("assistant", "HPCG 编译完成成功。"),
        ];
        let steps = vec![step("verify", Some("verify"), "检查产物")];
        assert!(!turn_suppress_completed_replanning(&messages, true, &steps));
    }

    #[test]
    fn readonly_suppresses_probe_only_replan_after_early_stop_allowed() {
        let messages = vec![
            msg("user", "分析当前目录"),
            tool_env("list_tree", "list tree", "list tree: ."),
            msg("assistant", "分析完成，总结如下。"),
        ];
        let steps = vec![step("summary", Some("summary"), "最终汇报")];
        assert!(turn_suppress_completed_replanning(&messages, true, &steps));
    }

    #[test]
    fn injects_when_tools_succeeded_but_assistant_empty() {
        let msgs = vec![
            Message::user_only("编译 hello"),
            tool_env(
                "run_command",
                "make",
                "命令：make\n退出码：0\n标准输出：\nBuilt target hello",
            ),
            Message::assistant_only(""),
        ];
        let fb = outer_loop_missing_final_answer_feedback_if_needed(&msgs, &msgs[2], 0);
        assert!(fb.is_some());
        assert!(fb.unwrap().contains("编排纠偏"));
    }

    #[test]
    fn rolling_horizon_build_early_stop_when_last_step_acceptance_passes() {
        use crate::agent::plan_artifact::PlanStepAcceptance;
        use crate::types::MessageContent;

        let t_step = |stdout: &str| Message {
            role: "tool".to_string(),
            content: Some(MessageContent::Text(format!(
                "退出码：0\n标准输出：\n{stdout}\n"
            ))),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some("run_command".to_string()),
            tool_call_id: None,
        };
        let messages = vec![
            Message::user_only("编译 hello"),
            Message::user_staged_step_injection("### 分步 1/1\n- id: build\n- 描述: 构建"),
            t_step("[100%] Built target hello"),
            Message::assistant_only("构建步骤完成。"),
        ];
        let acceptance = PlanStepAcceptance {
            expect_exit_code: Some(0),
            expect_stdout_contains: Some("Built target hello".to_string()),
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: None,
        };
        assert!(!turn_early_stop_allowed(&messages));
        assert!(turn_staged_rolling_horizon_early_stop_allowed(
            &messages,
            Some(&acceptance),
            std::path::Path::new("/tmp"),
        ));
    }

    #[test]
    fn staged_step_window_tool_helper_matches_step_verifier_window() {
        use crate::types::tool_messages_in_staged_step_window;

        let messages = vec![
            Message::user_staged_step_injection("### 分步 1/1"),
            tool_env("run_command", "a", "退出码：0\n标准输出：\na\n"),
            Message::user_only("next"),
            tool_env("run_command", "b", "退出码：0\n标准输出：\nb\n"),
        ];
        assert_eq!(tool_messages_in_staged_step_window(&messages, 0).len(), 1);
    }
}
