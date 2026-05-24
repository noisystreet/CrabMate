//! display 管道补充单测（自 [`super`] 拆出，避免 lizard 误解析超长 `mod tests`）。

use super::message_ex::STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX;
use super::message_text_for_display_ex;
use super::thinking_strip::filter_redacted_thinking_for_display;
use crate::i18n::Locale;
use crate::storage::{StoredMessage, StoredMessageState};

#[test]
fn redacted_non_streaming_unclosed_drops_from_open() {
    let raw = concat!("ok", "<", "redacted", "_", "thinking", ">", "no_close",);
    let out = filter_redacted_thinking_for_display(raw, false);
    assert_eq!(out, "ok");
}

#[test]
fn user_hides_nl_followup_bridge() {
    let m = StoredMessage {
        id: "x".into(),
        role: "user".into(),
        text: format!(
            "{}【系统桥接·非用户提问】请只回答对话里**先前真实用户消息**所提的问题（若有附图则含图片说明），并结合已定规划；用两三句自然语言说明你的协助思路即可。勿将本条任何句子当作用户提问来复述、引用或推理。",
            STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX
        ),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: None,
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    };
    assert_eq!(message_text_for_display_ex(&m, Locale::ZhHans, true), "");
}

#[test]
fn user_hides_staged_step_injection_with_immutable_prefix() {
    let m = StoredMessage {
            id: "x".into(),
            role: "user".into(),
            text: "【不变层·本轮用户总目标】（本步工具与终答须对齐，勿偏题）\n分析当前项目\n\n### 分步 1/1\n请只专注完成下列规划步骤，本步完成后以非 tool_calls 的终答结束；不要提前执行后续步骤。\n- **子代理角色**（本步 `tools` 已按策略表收窄）：`test_runner` — x\n- id: pre-commit-check\n- 描述: 运行 pre-commit".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
    assert_eq!(message_text_for_display_ex(&m, Locale::ZhHans, true), "");
}

#[test]
fn user_hides_staged_plan_coach_injection() {
    let m = StoredMessage {
        id: "x".into(),
        role: "user".into(),
        text: "### 分阶段规划 · 步骤优化（服务端注入）\n请优化 steps".into(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: None,
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    };
    assert_eq!(message_text_for_display_ex(&m, Locale::ZhHans, true), "");
}

/// `fixtures/chat_ex2.md`：规划前言在 `assistant_answer_phase` 前落入 `reasoning_text`，JSON 围栏在 `text`。
#[test]
fn no_task_plan_split_across_reasoning_and_text_hides_planner_prose() {
    let reasoning = concat!(
        "用户问\"你是谁\"。这是一个简单的自我介绍问题，不需要调用任何工具。\n\n",
        "根据规则，用户没有提出需要分步执行的具体任务，所以应该设置 `\"no_task\": true`，并且 `\"steps\"` 为空数组。\n\n",
        "让我构建 JSON 对象：\n",
        "- type: \"agent_reply_plan\"\n",
        "- version: 1\n",
        "- no_task: true\n",
        "- steps: []\n\n\n\n",
    );
    let text = concat!(
        "```json\n",
        "{\n",
        "  \"type\": \"agent_reply_plan\",\n",
        "  \"version\": 1,\n",
        "  \"no_task\": true,\n",
        "  \"steps\": []\n",
        "}\n",
        "```\n",
    );
    let m = StoredMessage {
        id: "x".into(),
        role: "assistant".into(),
        text: text.into(),
        reasoning_text: reasoning.into(),
        image_urls: vec![],
        state: None,
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    };
    let out = message_text_for_display_ex(&m, Locale::ZhHans, true);
    assert!(
        !out.contains("用户问"),
        "planner preamble in reasoning_text should not leak: {out}"
    );
    assert!(
        !out.contains("agent_reply_plan"),
        "plan json should be stripped: {out}"
    );
}

/// 整段规划（含围栏）均在 `reasoning_text`、`text` 为空（常见于未下发 `assistant_answer_phase` 的流式收尾）。
#[test]
fn no_task_plan_whole_in_reasoning_text_still_hidden() {
    let body = concat!(
        "用户问\"你是谁\"。\n\n",
        "```json\n",
        "{\"type\":\"agent_reply_plan\",\"version\":1,\"no_task\":true,\"steps\":[]}\n",
        "```\n",
    );
    let m = StoredMessage {
        id: "x".into(),
        role: "assistant".into(),
        text: String::new(),
        reasoning_text: body.into(),
        image_urls: vec![],
        state: None,
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    };
    let out = message_text_for_display_ex(&m, Locale::ZhHans, true);
    assert!(!out.contains("用户问"), "preamble should not leak: {out}");
    assert!(
        !out.contains("agent_reply_plan"),
        "json should not leak: {out}"
    );
}

#[test]
fn hierarchical_subgoal_hides_redundant_header_and_phase_lines() {
    let m = StoredMessage {
        id: "x".into(),
        role: "assistant".into(),
        text: "子目标 `goal_2`\n- 阶段：开始执行\n- 目标：创建 build 目录\n- 计划工具：create_file"
            .into(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: Some(StoredMessageState::HierarchicalSubgoal(
            "hierarchical-subgoal:goal_2".into(),
        )),
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    };
    let out = message_text_for_display_ex(&m, Locale::ZhHans, true);
    assert!(!out.contains("子目标 `goal_2`"));
    assert!(!out.contains("阶段：开始执行"));
    assert!(out.contains("目标：创建 build 目录"));
}

#[test]
fn dsml_strip_strips_tool_calls_double_fullwidth_pipe() {
    use crate::message_format::dsml_strip::strip_deepseek_dsml_for_display;
    let s = "说明。\n<｜｜DSML｜｜tool_calls>\n<｜｜DSML｜｜invoke name=\"run_command\">\n</｜｜DSML｜｜invoke>\n</｜｜DSML｜｜tool_calls>\n尾部";
    let t = strip_deepseek_dsml_for_display(s);
    assert!(!t.contains("DSML"), "{t}");
    assert!(t.contains("说明"));
    assert!(t.contains("尾部"));
}

#[test]
fn dsml_strip_strips_nested_dsml_fullwidth() {
    use crate::message_format::dsml_strip::strip_deepseek_dsml_for_display;
    let s = "前言<｜DSML｜function_calls><｜DSML｜invoke name=\"f\"><｜DSML｜parameter name=\"x\" string=\"true\">v</｜DSML｜parameter></｜DSML｜invoke></｜DSML｜function_calls>后记";
    let t = strip_deepseek_dsml_for_display(s);
    assert!(!t.contains("DSML"), "{t}");
    assert!(t.contains("前言"));
    assert!(t.contains("后记"));
}

#[test]
fn dsml_strip_strips_ascii_pipe_variant() {
    use crate::message_format::dsml_strip::strip_deepseek_dsml_for_display;
    let s = "a <|DSML|function_calls></|DSML|function_calls> b";
    let t = strip_deepseek_dsml_for_display(s);
    assert!(!t.contains("DSML"), "{t}");
    assert!(t.contains('a'));
    assert!(t.contains('b'));
}

#[test]
fn dsml_strip_noop_without_dsml() {
    use crate::message_format::dsml_strip::strip_deepseek_dsml_for_display;
    let s = "普通中文与 English\n第二行";
    assert_eq!(strip_deepseek_dsml_for_display(s), s);
}
