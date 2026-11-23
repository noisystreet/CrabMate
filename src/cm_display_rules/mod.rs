//! 聊天区展示层共用的字符串规则（无 UI / 无 I/O）。
//!
//! 与主仓 `src/cm_runtime/message_display.rs`、前端 `message_format/display/message_ex/parts.rs`
//! 对齐；金样见仓库根 `fixtures/display_hide_user_golden.jsonl`。

/// 无工具规划轮 tool_calls 拒绝后的一次性重写约束 user 首行（同一来源见 `cm_types`）。
pub use crate::cm_types::STAGED_PLANNER_TOOL_CALL_REJECT_CONTENT_PREFIX as STAGED_PLANNER_TOOL_CALL_REJECT_PREFIX;

/// 外循环纠偏 user 首行（截断 / 终答缺失纠偏共用；与 `turn_completion` 对齐）。
pub const OUTER_LOOP_ORCHESTRATION_FEEDBACK_PREFIX: &str = "【编排纠偏】";

/// 长期记忆召回注入正文前缀（为 `long_term_memory::format_ltm_injection_body` 的前缀；无 `user.name` 时的兜底识别）。
pub const LONG_TERM_MEMORY_INJECTION_CONTENT_PREFIX: &str = "以下为与当前问题可能相关的长期记忆";

/// LLM 中间段摘要正文前缀（同一来源见 `cm_types`）。
pub use crate::cm_types::CONTEXT_SUMMARY_INJECTION_CONTENT_PREFIX;

#[must_use]
pub fn is_long_term_memory_injected_user_content(s: &str) -> bool {
    s.trim_start()
        .starts_with(LONG_TERM_MEMORY_INJECTION_CONTENT_PREFIX)
}

#[must_use]
pub fn is_context_summary_injected_user_content(s: &str) -> bool {
    s.trim_start()
        .starts_with(CONTEXT_SUMMARY_INJECTION_CONTENT_PREFIX)
}

#[must_use]
pub fn is_planner_tool_call_reject_injected_user_content(s: &str) -> bool {
    s.trim_start()
        .starts_with(STAGED_PLANNER_TOOL_CALL_REJECT_PREFIX)
}

#[must_use]
pub fn is_staged_patch_feedback_user_content(s: &str) -> bool {
    s.trim_start().starts_with("### 分阶段规划 · 步级反馈")
}

/// 规划重写注入 user 的**稳定机器标记**（生产与隐藏判定共用同一常量）。
///
/// 生产侧 [`crate::cm_agent::plan_rewrite::plan_rewrite_user_text_base`] 在正文开头写入该标记；
/// 判定侧只认标记，不再逐字匹配自然语言句子——句子一改，隐藏就会静默失效（#1478）。
pub const PLAN_REWRITE_INJECTION_MARKER: &str = "【规划重写】";

/// 规划重写注入的隐藏判定：**只认机器标记**。
///
/// 三条「提示词原句」历史兼容（`你的最终回答缺少**结构化规划**` / `crabmate_plan_semantic_feedback` /
/// `侧向校验认为你的 **agent_reply_plan**`）已于 #1494 删除：所有注入路径都以
/// `cm_agent::plan_rewrite::plan_rewrite_user_text_base` 为底座，标记随继承覆盖
/// （含 `_with_issue` 与语义反馈两种包装），因此不需要再按句子兜底。
///
/// 现在**无标记即不隐藏** —— 由 `plan_rewrite_hidden_only_by_marker` 与
/// `fixtures/display_hide_user_golden.jsonl` 的双向样例钉住。
#[must_use]
pub fn is_plan_rewrite_injected_user_content(s: &str) -> bool {
    s.contains(PLAN_REWRITE_INJECTION_MARKER)
}

/// Web / TUI 默认：服务端注入 user 在聊天区隐藏。
#[must_use]
pub fn user_message_should_hide_for_chat_display(s: &str) -> bool {
    is_planner_tool_call_reject_injected_user_content(s)
        || is_staged_patch_feedback_user_content(s)
        || is_plan_rewrite_injected_user_content(s)
        || is_long_term_memory_injected_user_content(s)
        || is_context_summary_injected_user_content(s)
        || s.trim_start()
            .starts_with(OUTER_LOOP_ORCHESTRATION_FEEDBACK_PREFIX)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    /// #1494：历史兼容的三条「提示词原句」已删除 —— 判定**只看机器标记**。
    ///
    /// 负向半边是重点：无标记时一律不隐藏，否则「用户正文恰好提到这些词」会被误吞。
    #[test]
    fn plan_rewrite_hidden_only_by_marker() {
        assert!(is_plan_rewrite_injected_user_content(
            "【规划重写】你的最终回答缺少或未通过**结构化规划**校验。"
        ));

        for legacy_without_marker in [
            // 升级前注入的底座（无标记）
            "你的最终回答缺少**结构化规划**。请加入 agent_reply_plan JSON",
            // 语义反馈正文（真实注入里它后面会拼上带标记的底座）
            "侧向校验认为你的 **agent_reply_plan** 与**最近工具执行结果**存在明显矛盾。",
            // 机器可读行本身
            "{\"kind\":\"crabmate_plan_semantic_feedback\",\"version\":1}",
        ] {
            assert!(
                !is_plan_rewrite_injected_user_content(legacy_without_marker),
                "无标记不应隐藏: {legacy_without_marker}"
            );
        }

        // 用户自己提到这些字段名，也不得被聊天区吞掉。
        assert!(!user_message_should_hide_for_chat_display(
            "请把 crabmate_plan_semantic_feedback 这个字段名换成别的"
        ));
    }

    #[test]
    fn display_hide_user_golden() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = manifest.join("fixtures/display_hide_user_golden.jsonl");
        let raw =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for (line_no, line) in raw.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let mut parts = t.splitn(3, '\t');
            let label = parts.next().unwrap_or("?");
            let body = parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing body", line_no + 1))
                .replace("\\n", "\n");
            let expect_hidden = parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing expect", line_no + 1));
            let expect_hidden = match expect_hidden {
                "hide" => true,
                "show" => false,
                other => panic!("line {} ({}): bad expect {other:?}", line_no + 1, label),
            };
            let got = user_message_should_hide_for_chat_display(body.as_str());
            assert_eq!(got, expect_hidden, "line {} ({})", line_no + 1, label);
        }
    }
}
