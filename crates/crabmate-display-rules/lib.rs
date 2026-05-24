//! 聊天区展示层共用的字符串规则（无 UI / 无 I/O）。
//!
//! 与主仓 `src/runtime/message_display.rs`、前端 `message_format/display/message_ex/parts.rs`
//! 对齐；金样见仓库根 `fixtures/display_hide_user_golden.jsonl`。

/// 与 `src/runtime/plan_section.rs` 中同名常量一致。
pub const STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX: &str = "### CrabMate·NL补全\n";

/// 分阶段 coach / 补丁 **user** 首行标记（与 `staged/sse.rs` 注入对齐）。
pub const STAGED_PLAN_SYSTEM_COACH_PREFIX: &str = "### 分阶段规划 ·";

/// 步骤优化轮 user 注入标记（与 `plan_optimizer::STAGED_PLAN_OPTIMIZER_COACH_MARK` 一致）。
pub const STAGED_PLAN_OPTIMIZER_COACH_MARK: &str = "### 分阶段规划 · 步骤优化（服务端注入）";

pub const STAGED_PLAN_ENSEMBLE_B_COACH_MARK: &str = "### 分阶段规划 · 逻辑规划员 B（服务端注入）";
pub const STAGED_PLAN_ENSEMBLE_C_COACH_MARK: &str = "### 分阶段规划 · 逻辑规划员 C（服务端注入）";
pub const STAGED_PLAN_ENSEMBLE_MERGE_COACH_MARK: &str = "### 分阶段规划 · 合并多规划（服务端注入）";

/// 无工具规划轮 tool_calls 拒绝后的一次性重写约束 user 首行（与 `types::STAGED_PLANNER_TOOL_CALL_REJECT_CONTENT_PREFIX` 一致）。
pub const STAGED_PLANNER_TOOL_CALL_REJECT_PREFIX: &str =
    "### 规划轮约束提醒（code=PLANNER_TOOL_CALL_REJECTED）";

/// 分步注入 user 的正文特征（不含 `SHOW_STAGED_STEP_USER_BOILERPLATE_IN_CHAT` 开关）。
#[must_use]
pub fn is_staged_step_injection_user_pattern(s: &str) -> bool {
    let t = s.trim_start();
    t.contains("\n- id:")
        && t.contains("\n- 描述:")
        && (t.contains("### 分步 ") || t.starts_with("【分步执行"))
}

#[must_use]
pub fn is_staged_nl_followup_bridge_user_content(s: &str) -> bool {
    s.trim_start()
        .contains(STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX)
}

#[must_use]
pub fn is_staged_plan_coach_injected_user_content(s: &str) -> bool {
    s.trim_start().contains(STAGED_PLAN_SYSTEM_COACH_PREFIX)
}

#[must_use]
pub fn is_ensemble_injected_user_content(s: &str) -> bool {
    s.contains(STAGED_PLAN_ENSEMBLE_B_COACH_MARK)
        || s.contains(STAGED_PLAN_ENSEMBLE_C_COACH_MARK)
        || s.contains(STAGED_PLAN_ENSEMBLE_MERGE_COACH_MARK)
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

#[must_use]
pub fn is_plan_rewrite_injected_user_content(s: &str) -> bool {
    s.contains("你的最终回答缺少**结构化规划**")
        || s.contains("crabmate_plan_semantic_feedback")
        || s.contains("侧向校验认为你的 **agent_reply_plan**")
}

/// Web / TUI 默认：分步注入、NL 桥接、coach user 均在聊天区隐藏。
#[must_use]
pub fn user_message_should_hide_for_chat_display(s: &str) -> bool {
    is_staged_step_injection_user_pattern(s)
        || is_staged_nl_followup_bridge_user_content(s)
        || is_staged_plan_coach_injected_user_content(s)
        || is_planner_tool_call_reject_injected_user_content(s)
        || is_staged_patch_feedback_user_content(s)
        || is_plan_rewrite_injected_user_content(s)
        || is_ensemble_injected_user_content(s)
        || s.contains(STAGED_PLAN_OPTIMIZER_COACH_MARK)
}

/// 无 `user.name` 时用于 [`crate::types`] 识别服务端注入 user（展示层 + 落盘过滤）。
#[must_use]
pub fn is_server_injected_user_content_for_storage(s: &str) -> bool {
    user_message_should_hide_for_chat_display(s)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn display_hide_user_golden() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root");
        let path = root.join("fixtures/display_hide_user_golden.jsonl");
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
