//! 回合级「目标完成 / 早停 / 冗余工具抑制 / 终答纠偏 / 步后抑规划」共用判定。
//!
//! 纯判定核（[`TurnCompletionDecision`] / evaluate_*）已下沉 **`crate::cm_agent`**；本文件保留
//! 外循环纠偏文案与依赖 `tool_result` 信封的终答空答检测。

#[cfg(test)]
#[path = "turn_completion_golden.rs"]
mod turn_completion_golden;

#[cfg(test)]
pub(crate) use crate::agent::agent_turn::policy::turn_completion_decision::evaluate_turn_suppress_replanning;
pub(crate) use crate::agent::agent_turn::policy::turn_completion_decision::{
    TurnCompletionDecision, evaluate_turn_early_stop, evaluate_turn_redundant_tools,
    log_turn_completion_decision,
};

pub(crate) use crate::agent::agent_turn::policy::completion_suppression::redundant_tool_names_for_log;

pub(crate) use crate::cm_display_rules::OUTER_LOOP_BUILD_IDLE_ORCHESTRATION_PREFIX;

#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use crate::agent::agent_turn::policy::completion_suppression::{
    plan_steps_are_redundant_after_completion, tool_call_is_redundant_after_completion,
    tool_calls_are_redundant_after_completion,
};

use crate::types::{Message, ToolCall, last_real_user_message_index, message_content_as_str};

/// 最多注入次数，避免与外层迭代上限死磕。
pub(crate) const OUTER_LOOP_MISSING_FINAL_ANSWER_FEEDBACK_MAX: u32 = 2;

/// 低于该字符数（Unicode scalar）视为「无可见终答」。
pub(crate) const OUTER_LOOP_MISSING_FINAL_ANSWER_MIN_CHARS: usize = 24;

/// 截断纠偏最多注入次数，避免长输出反复触发与外层迭代上限死磕。
pub(crate) const OUTER_LOOP_TRUNCATED_FEEDBACK_MAX: u32 = 2;

/// 任务级证据已 Satisfied 时是否允许**提前停轮**（规划步滚动视界与子 Agent 外循环共用）。
pub(crate) fn turn_early_stop_allowed(messages: &[Message]) -> bool {
    evaluate_turn_early_stop(messages).is_allow()
}

/// 活跃目标已有完成证据且允许早停时，是否应静默丢弃本轮探针类 / 重复 `run_command` 工具调用。
pub(crate) fn turn_redundant_tools_after_completion_allowed(
    tool_calls: &[ToolCall],
    messages: &[Message],
) -> bool {
    evaluate_turn_redundant_tools(tool_calls, messages).is_allow()
}

/// 步后重规划：目标已 Satisfied 且新 `steps` 仅为探针/总结时是否应抑制下一轮无工具规划。
///
/// 生产 outer_loop 尚未接线；仅单测与金样使用。
#[cfg(test)]
pub(crate) fn turn_suppress_completed_replanning(
    messages: &[Message],
    entered_from_step_execution_round: bool,
    steps: &[crate::agent::plan_artifact::PlanStepV1],
) -> bool {
    evaluate_turn_suppress_replanning(messages, entered_from_step_execution_round, steps).is_allow()
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
        prefix = OUTER_LOOP_BUILD_IDLE_ORCHESTRATION_PREFIX
    )
}

/// 若应注入纠偏 user 并继续外循环，返回 `Some(feedback)`。
pub(crate) fn outer_loop_missing_final_answer_feedback_if_needed(
    messages: &[Message],
    assistant: &Message,
    feedback_injected_count: u32,
) -> Option<String> {
    let decision = if feedback_injected_count >= OUTER_LOOP_MISSING_FINAL_ANSWER_FEEDBACK_MAX {
        TurnCompletionDecision::DenyMissingFinalAnswerFeedback {
            reason: "feedback_budget_exhausted",
        }
    } else if !outer_loop_assistant_lacks_visible_final_answer(assistant) {
        TurnCompletionDecision::DenyMissingFinalAnswerFeedback {
            reason: "assistant_has_visible_answer",
        }
    } else if !outer_loop_window_has_any_successful_tool(messages) {
        TurnCompletionDecision::DenyMissingFinalAnswerFeedback {
            reason: "no_successful_tool_in_window",
        }
    } else {
        TurnCompletionDecision::AllowMissingFinalAnswerFeedback
    };
    log_turn_completion_decision(decision, "missing_final_answer_feedback");
    if decision.is_allow() {
        Some(outer_loop_missing_final_answer_feedback_body())
    } else {
        None
    }
}

pub(crate) fn outer_loop_truncated_feedback_body() -> String {
    format!(
        "{prefix}上一轮助手输出因达到模型长度上限或流式中断而被**截断**，终答不完整。\
         请基于已有上下文**继续**并给出完整、可验收的终答：若任务已完成，直接输出最终结论；\
         若仍需执行，请通过 `tool_calls` 继续；避免重复已输出内容，必要时分段或精炼。",
        prefix = OUTER_LOOP_BUILD_IDLE_ORCHESTRATION_PREFIX
    )
}

/// 若本轮 assistant 输出被上游截断且仍有纠偏预算，返回 `Some(feedback)`。
///
/// 触发仅依赖层 1 的截断信号（[`crate::per_turn_flight::PerTurnFlight::last_assistant_finish_truncated`]）；
/// 与正文长度无关——截断常伴随**已有较长但未完成**的正文，故不复用终答缺失的字符数启发式。
pub(crate) fn outer_loop_truncated_feedback_if_needed(
    truncated: bool,
    feedback_injected_count: u32,
) -> Option<String> {
    if !truncated || feedback_injected_count >= OUTER_LOOP_TRUNCATED_FEEDBACK_MAX {
        return None;
    }
    Some(outer_loop_truncated_feedback_body())
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
        assert_eq!(
            evaluate_turn_early_stop(&messages).deny_reason(),
            Some("build_or_test_intent")
        );
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
    }

    #[test]
    fn injects_truncated_feedback_when_truncated_within_budget() {
        let fb = outer_loop_truncated_feedback_if_needed(true, 0);
        let fb = fb.expect("截断且预算内应注入纠偏");
        assert!(fb.contains("截断"));
        assert!(fb.contains(OUTER_LOOP_BUILD_IDLE_ORCHESTRATION_PREFIX));
    }

    #[test]
    fn skips_truncated_feedback_when_not_truncated_or_budget_exhausted() {
        assert!(outer_loop_truncated_feedback_if_needed(false, 0).is_none());
        assert!(
            outer_loop_truncated_feedback_if_needed(true, OUTER_LOOP_TRUNCATED_FEEDBACK_MAX)
                .is_none()
        );
    }
}
