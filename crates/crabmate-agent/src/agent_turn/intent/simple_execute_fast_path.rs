//! 简单编译/构建类 Execute：跳过滚动分阶段，走外循环以降低 replan 与 L2 重复成本。

use crate::intent_pipeline::{IntentAction, IntentDecision};

fn task_has_build_keyword(task: &str) -> bool {
    let t = task.to_lowercase();
    [
        "编译",
        "构建",
        "build",
        "compile",
        "make",
        "cmake",
        "cargo check",
        "cargo build",
        "cargo test",
        "pytest",
        "npm test",
        "npm run build",
    ]
    .iter()
    .any(|k| t.contains(k))
}

fn task_implies_multi_step_or_advisory(task: &str) -> bool {
    let t = task.to_lowercase();
    [
        "架构",
        "重构",
        "多个模块",
        "全仓库",
        "整体设计",
        "开 pr",
        "pull request",
        "提交并",
        "并提交",
        "再提交",
        "文档",
        "readme",
        "分析并",
        "梳理并",
    ]
    .iter()
    .any(|k| t.contains(k))
}

/// 命中时 [`super::super::staged_planning_gate`] 应拒绝分阶段并走 freeform 外循环。
pub fn should_bypass_staged_for_simple_build_execute(
    task: &str,
    decision: &IntentDecision,
) -> bool {
    if !matches!(decision.action, IntentAction::Execute) {
        return false;
    }
    if !task_has_build_keyword(task) {
        return false;
    }
    if task_implies_multi_step_or_advisory(task) {
        return false;
    }
    let primary = decision.primary_intent.as_str();
    primary.starts_with("execute.run_test_build")
        || (primary.starts_with("execute.code_change") && task_has_build_keyword(task))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_pipeline::IntentDecision;
    use crate::intent_router::IntentKind;

    fn exec_decision(primary: &str) -> IntentDecision {
        IntentDecision {
            kind: IntentKind::Execute,
            primary_intent: primary.to_string(),
            secondary_intents: vec![],
            confidence: 0.9,
            abstain: false,
            need_clarification: false,
            action: IntentAction::Execute,
        }
    }

    #[test]
    fn simple_cmake_cpp_bypasses_staged() {
        assert!(should_bypass_staged_for_simple_build_execute(
            "帮我编写一个简单c++程序，然后使用cmake编译执行",
            &exec_decision("execute.run_test_build"),
        ));
    }

    #[test]
    fn git_pr_task_not_fast_path() {
        assert!(!should_bypass_staged_for_simple_build_execute(
            "编译通过后提交并开 PR",
            &exec_decision("execute.git_ops"),
        ));
    }

    #[test]
    fn readonly_qa_not_fast_path() {
        let mut d = exec_decision("execute.read_inspect");
        d.action = IntentAction::DirectReply("只读".into());
        assert!(!should_bypass_staged_for_simple_build_execute(
            "列出源文件",
            &d
        ));
    }
}
