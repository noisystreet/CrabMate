//! 任务级 Satisfied 是否允许外循环/staged 提前停轮（构建/测试类须走完规划步）。

use crate::types::Message;

use super::verify::{
    GoalCompletionEvidenceCheck, check_active_user_goal_completion_evidence,
    generic_task_intent_implies_build_or_test,
};

/// 任务级证据已 Satisfied 时是否允许**提前停轮**（规划步滚动视界与子 Agent 外循环共用）。
///
/// 构建/测试类任务须走完规划步（含 `test_runner` / 有效 `acceptance`），不得仅凭启发式早停。
pub(crate) fn task_level_satisfied_allows_early_stop(messages: &[Message]) -> bool {
    if !matches!(
        check_active_user_goal_completion_evidence(messages),
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
        assert!(!task_level_satisfied_allows_early_stop(&messages));
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
        assert!(!task_level_satisfied_allows_early_stop(&messages));
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
        assert!(task_level_satisfied_allows_early_stop(&messages));
    }
}
