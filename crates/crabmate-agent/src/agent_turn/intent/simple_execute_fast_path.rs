//! 简单编译/构建类 Execute：已禁用——现在全部走分阶段规划（Staged），以改善气泡分离。
//!
//! 此前此类任务跳过滚动分阶段走 Freeform 外循环，导致模型将工具调用间解说全部堆积到
//! 最后一条消息，产生"巨泡"。如需恢复旧行为，让此函数返回原逻辑即可。

use crate::intent_pipeline::IntentDecision;

/// 始终返回 `false`：编译/构建类任务不再绕过 staged 门控。
/// 保留签名和模块以最小化 diff，实际逻辑已禁用。
pub fn should_bypass_staged_for_simple_build_execute(
    _task: &str,
    _decision: &IntentDecision,
) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_pipeline::{IntentAction, IntentDecision};
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

    /// Fast path 已禁用：所有构建任务均不绕过 staged。
    #[test]
    fn fast_path_disabled_always_false() {
        assert!(!should_bypass_staged_for_simple_build_execute(
            "帮我编写一个简单c++程序，然后使用cmake编译执行",
            &exec_decision("execute.run_test_build"),
        ));
        assert!(!should_bypass_staged_for_simple_build_execute(
            "编译 hpcg",
            &exec_decision("execute.run_test_build"),
        ));
        assert!(!should_bypass_staged_for_simple_build_execute(
            "cargo build",
            &exec_decision("execute.code_change"),
        ));
    }

    #[test]
    fn non_execute_action_returns_false() {
        let mut d = exec_decision("execute.read_inspect");
        d.action = IntentAction::DirectReply("只读".into());
        assert!(!should_bypass_staged_for_simple_build_execute(
            "列出源文件",
            &d
        ));
    }
}
