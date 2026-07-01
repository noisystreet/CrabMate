//! 回合级「目标完成 / 早停 / 冗余工具抑制」共用判定（外循环、滚动视界、completion_suppression）。

use crate::types::{Message, ToolCall};

use super::completion_suppression::tool_calls_are_redundant_when_goal_satisfied;
use super::task_level_evidence::{
    GoalCompletionEvidenceCheck, check_active_user_goal_completion_evidence,
    generic_task_intent_implies_build_or_test,
};

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

#[cfg(test)]
mod tests {
    use super::*;
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
}
