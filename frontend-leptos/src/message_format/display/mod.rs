//! 助手/用户/系统消息的展示管道（`agent_reply_plan`、思维链过滤等）。
//!
//! 子模块：[`plan_fence`]（规划 JSON / 围栏）、[`thinking_strip`]（思维链标签）、[`message_ex`]（按角色拼正文）。

mod message_ex;
mod plan_fence;
mod thinking_strip;

pub(crate) use message_ex::message_text_for_display_ex;
pub(crate) use plan_fence::{
    agent_reply_plan_step_descriptions_from_assistant, assistant_text_for_display,
    stored_message_is_staged_planner_round,
};
pub(crate) use thinking_strip::{
    assistant_thinking_body_and_answer_raw, filter_assistant_thinking_markers_for_display,
};

#[cfg(test)]
mod tests {
    use super::super::plain::collapse_consecutive_blank_lines;
    use super::message_ex::STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX;
    use super::message_text_for_display_ex;
    use super::plan_fence::assistant_text_for_display;
    use super::plan_fence::stored_message_is_staged_planner_round;
    use super::thinking_strip::{
        assistant_thinking_body_and_answer_raw, filter_assistant_thinking_markers_for_display,
        filter_redacted_thinking_for_display,
    };
    use crate::i18n::Locale;
    use crate::storage::StoredMessage;

    /// Embedded copy of `fixtures/chat_resp1.md` (redacted blocks + `agent_reply_plan` fence).
    const CHAT_RESP1_FIXTURE: &str = include_str!("../../../fixtures/chat_resp1.md");

    #[test]
    fn collapse_consecutive_blank_lines_merges_runs() {
        assert_eq!(collapse_consecutive_blank_lines("a\n\n\nb"), "a\n\nb");
        assert_eq!(collapse_consecutive_blank_lines("\n\nfoo"), "foo");
        assert_eq!(collapse_consecutive_blank_lines("x\n  \n\t\ny"), "x\n\ny");
    }

    #[test]
    fn hide_inline_agent_reply_plan_json_fence() {
        let raw = r#"```json{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}```"#;
        let out = assistant_text_for_display(raw, true, Locale::ZhHans, true);
        assert!(
            !out.contains("agent_reply_plan"),
            "raw agent_reply_plan json should be filtered: {out}"
        );
        assert!(
            !out.contains("```"),
            "agent_reply_plan fence should be stripped: {out}"
        );
    }

    #[test]
    fn no_task_empty_plan_has_non_empty_fallback() {
        let raw = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#;
        let out = assistant_text_for_display(raw, false, Locale::ZhHans, true);
        assert!(
            !out.trim().is_empty(),
            "filtered plan text should not become empty"
        );
    }

    #[test]
    fn keep_answer_after_fenced_plan_json() {
        let raw = r#"```json{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}```最终结论：已完成。"#;
        let out = assistant_text_for_display(raw, false, Locale::ZhHans, true);
        assert!(
            out.contains("最终结论"),
            "tail answer should be kept: {out}"
        );
        assert!(
            !out.contains("agent_reply_plan"),
            "raw plan json should be hidden: {out}"
        );
    }

    #[test]
    fn keep_answer_after_unfenced_plan_json_prefix() {
        let raw = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}最终结论：继续执行。"#;
        let out = assistant_text_for_display(raw, false, Locale::ZhHans, true);
        assert!(
            out.contains("最终结论"),
            "tail answer should be kept: {out}"
        );
        assert!(
            !out.contains("agent_reply_plan"),
            "raw plan json should be hidden: {out}"
        );
    }

    #[test]
    fn drops_prose_before_first_agent_reply_plan_fence() {
        let preamble = "模型规划说明（不应展示）\n\n";
        let raw = format!(
            r#"{preamble}```json{{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}}```最终结论：保留。"#,
            preamble = preamble
        );
        let out = assistant_text_for_display(&raw, false, Locale::ZhHans, true);
        assert!(
            out.contains("最终结论"),
            "tail after fence should be kept: {out}"
        );
        assert!(
            !out.contains("模型规划说明"),
            "preamble before first plan fence should be dropped: {out}"
        );
    }

    #[test]
    fn strips_qwen_think_block_in_combined_filter() {
        let raw = concat!(
            "你好",
            "<",
            "think",
            ">",
            "内省正文",
            "</",
            "think",
            ">",
            "尾",
        );
        let out = filter_assistant_thinking_markers_for_display(raw, false);
        assert_eq!(out, "你好尾");
    }

    #[test]
    fn strips_two_think_blocks_in_combined_filter() {
        let o = concat!("<", "think", ">");
        let c = concat!("</", "think", ">");
        let raw = format!("a{o}1{c}m{o}2{c}z");
        let out = filter_assistant_thinking_markers_for_display(&raw, false);
        assert_eq!(out, "amz");
    }

    fn assert_filtered_redacted_plan_export_body(out: &str) {
        let open = concat!("<", "redacted", "_", "thinking", ">");
        let close = concat!("</", "redacted", "_", "thinking", ">");
        assert!(
            !out.contains(open),
            "redacted open tag should be stripped:\n{out}"
        );
        assert!(
            !out.contains(close),
            "redacted close tag should be stripped:\n{out}"
        );
        assert!(
            !out.contains("agent_reply_plan"),
            "plan json should be hidden:\n{out}"
        );
        assert!(
            !out.contains("用户问"),
            "first redacted block body should be removed:\n{out}"
        );
        assert!(
            !out.contains("用户发送了"),
            "second redacted block body should be removed:\n{out}"
        );
        assert!(
            out.contains("CrabMate"),
            "visible prose should remain:\n{out}"
        );
        assert!(
            out.contains("有具体代码任务"),
            "tail prose before fence should remain:\n{out}"
        );
        assert!(
            out.contains("好的，我可以帮你"),
            "final answer line should remain:\n{out}"
        );
    }

    /// 工作区根目录 `chat_selection_20260410_230651.md`（可选）：与 `chat_resp1` 同形，但带 `## 助手` 导出标题；文件不存在时跳过。
    #[test]
    fn filter_chat_selection_export_fixture_md() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../chat_selection_20260410_230651.md");
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return;
        };
        let body = raw
            .strip_prefix("## 助手\n\n")
            .or_else(|| raw.strip_prefix("## 助手\r\n\r\n"))
            .unwrap_or(raw.as_str());
        let out = assistant_text_for_display(body, false, Locale::ZhHans, true);
        assert_filtered_redacted_plan_export_body(&out);
    }

    #[test]
    fn filter_chat_resp1_fixture_md() {
        let out =
            assistant_text_for_display(CHAT_RESP1_FIXTURE.trim(), false, Locale::ZhHans, true);
        assert_filtered_redacted_plan_export_body(&out);
    }

    #[test]
    fn filter_chat_resp1_message_text_for_display_ex_all_in_text() {
        let m = StoredMessage {
            id: "chat_resp1".into(),
            role: "assistant".into(),
            text: CHAT_RESP1_FIXTURE.to_string(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            created_at: 0,
        };
        let out = message_text_for_display_ex(&m, Locale::ZhHans, true);
        assert_filtered_redacted_plan_export_body(&out);
    }

    #[test]
    fn filter_chat_resp1_message_text_for_display_ex_split_after_first_redacted_block() {
        let needle = "</think>";
        let pos = CHAT_RESP1_FIXTURE
            .find(needle)
            .expect("chat_resp1.md must contain closing redacted_thinking");
        let split = pos + needle.len();
        let reasoning = &CHAT_RESP1_FIXTURE[..split];
        let text = CHAT_RESP1_FIXTURE[split..]
            .trim_start_matches(['\n', '\r'])
            .to_string();
        let m = StoredMessage {
            id: "chat_resp1-split".into(),
            role: "assistant".into(),
            text,
            reasoning_text: reasoning.to_string(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            created_at: 0,
        };
        let out = message_text_for_display_ex(&m, Locale::ZhHans, true);
        assert_filtered_redacted_plan_export_body(&out);
    }

    #[test]
    fn no_inline_split_when_disabled() {
        let raw = concat!("<", "think", ">", "x", "</", "think", ">", "y",);
        let (think, ans) = assistant_thinking_body_and_answer_raw("", raw, false);
        assert!(think.is_empty());
        assert_eq!(ans, raw);
    }

    #[test]
    fn assistant_text_passthrough_when_filters_off() {
        let raw = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#;
        let out = assistant_text_for_display(raw, false, Locale::ZhHans, false);
        assert_eq!(out, raw);
    }

    #[test]
    fn stored_message_is_staged_planner_round_detects_plan_in_text() {
        let m = StoredMessage {
            id: "1".into(),
            role: "assistant".into(),
            text: r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            created_at: 0,
        };
        assert!(stored_message_is_staged_planner_round(&m));
    }

    #[test]
    fn stored_message_is_staged_planner_round_detects_plan_in_reasoning_only() {
        let m = StoredMessage {
            id: "2".into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#
                .into(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            created_at: 0,
        };
        assert!(stored_message_is_staged_planner_round(&m));
    }

    #[test]
    fn stored_message_is_staged_planner_round_streaming_prefix() {
        let m = StoredMessage {
            id: "3".into(),
            role: "assistant".into(),
            text: r#"{"type":"agent_reply_plan","version":1"#.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some("loading".into()),
            is_tool: false,
            created_at: 0,
        };
        assert!(stored_message_is_staged_planner_round(&m));
    }

    #[test]
    fn stored_message_is_staged_planner_round_false_for_user_and_tool() {
        let user = StoredMessage {
            id: "u".into(),
            role: "user".into(),
            text: r#"{"type":"agent_reply_plan","version":1}"#.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            created_at: 0,
        };
        assert!(!stored_message_is_staged_planner_round(&user));
        let tool = StoredMessage {
            id: "t".into(),
            role: "assistant".into(),
            text: r#"{"type":"agent_reply_plan","version":1}"#.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: true,
            created_at: 0,
        };
        assert!(!stored_message_is_staged_planner_round(&tool));
    }

    #[test]
    fn system_display_strips_staged_plan_coach_header_and_prefixes_ordinal() {
        let m = StoredMessage {
            id: "s".into(),
            role: "system".into(),
            text: "### 分阶段规划 · 规划轮\n请仅根据用户消息.".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            created_at: 0,
        };
        let out = message_text_for_display_ex(&m, Locale::ZhHans, true);
        assert!(!out.contains("分阶段规划"), "out={out:?}");
        assert!(out.starts_with("1. 请仅"), "out={out:?}");
    }

    #[test]
    fn system_display_optimizer_coach_gets_ordinal_2() {
        let m = StoredMessage {
            id: "s2".into(),
            role: "system".into(),
            text: "### 分阶段规划 · 步骤优化（服务端注入）\nfoo".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            created_at: 0,
        };
        let out = message_text_for_display_ex(&m, Locale::ZhHans, true);
        assert!(out.starts_with("2. foo"), "out={out:?}");
    }

    #[test]
    fn splits_inline_thinking_from_assistant_content_when_no_reasoning_field() {
        let raw = concat!(
            "<",
            "think",
            ">",
            "plan here",
            "</",
            "think",
            ">",
            "\n\n**Answer** tail.",
        );
        let (think, ans) = assistant_thinking_body_and_answer_raw("", raw, true);
        assert_eq!(think.trim(), "plan here");
        assert!(ans.contains("Answer"));
        assert!(!ans.contains("plan here"));
    }

    #[test]
    fn stored_reasoning_text_wins_over_inline_tags() {
        let inline = concat!("`<", "think", ">`x`</", "think", ">`y");
        let (think, ans) = assistant_thinking_body_and_answer_raw("from_sse", inline, true);
        assert_eq!(think, "from_sse");
        assert_eq!(ans, inline);
    }

    #[test]
    fn strips_redacted_thinking_pair_complete() {
        let raw = concat!(
            "pre ", "<", "redacted", "_", "thinking", ">", "hidden", "</", "redacted", "_",
            "thinking", ">", " tail",
        );
        let out = filter_redacted_thinking_for_display(raw, false);
        assert_eq!(out, "pre  tail");
    }

    #[test]
    fn strips_two_redacted_thinking_pairs() {
        let o = concat!("<", "redacted", "_", "thinking", ">");
        let c = concat!("</", "redacted", "_", "thinking", ">");
        let raw = format!("a{o}x{c} b{o}y{c} c");
        let out = filter_redacted_thinking_for_display(&raw, false);
        assert_eq!(out, "a b c");
    }

    #[test]
    fn redacted_streaming_truncates_before_unclosed_block() {
        let raw = concat!("ok", "<", "redacted", "_", "thinking", ">", "partial",);
        let out = filter_redacted_thinking_for_display(raw, true);
        assert_eq!(out, "ok");
    }

    #[test]
    fn redacted_streaming_strips_suffix_matching_open_prefix() {
        let raw = "visible<redacted_thin";
        let out = filter_redacted_thinking_for_display(raw, true);
        assert_eq!(out, "visible");
    }

    #[test]
    fn strips_backtick_wrapped_redacted_pair() {
        let raw = concat!(
            "x", "`", "<", "redacted", "_", "thinking", ">", "`", "h", "`", "</", "redacted", "_",
            "thinking", ">", "`", "y",
        );
        let out = filter_redacted_thinking_for_display(raw, false);
        assert_eq!(out, "xy");
    }

    #[test]
    fn strips_case_insensitive_redacted_tags() {
        let raw = "<Redacted_Thinking>sec</redacted_THINKING>out";
        let out = filter_redacted_thinking_for_display(raw, false);
        assert_eq!(out, "out");
    }

    /// 反引号形态此前仅用 `find()` 精确匹配小写；上游若输出混合大小写，过滤器认不出开标签，Markdown 再剥掉裸标签后表现为「只剩正文」。
    #[test]
    fn strips_backtick_wrapped_redacted_when_tag_name_mixed_case() {
        let raw = concat!(
            "`", "<", "Redacted", "_", "Thinking", ">`", "SECRET", "`", "</", "REDACTED", "_",
            "THINKING", ">`", "tail",
        );
        let out = filter_redacted_thinking_for_display(raw, false);
        assert_eq!(out, "tail");
        assert!(!out.contains("SECRET"));
    }

    #[test]
    fn strips_mixed_backtick_open_and_plain_close_ci_redacted() {
        let raw = concat!(
            "`", "<", "Redacted", "_", "Thinking", ">`", "x", "</", "redacted", "_", "thinking",
            ">z",
        );
        let out = filter_redacted_thinking_for_display(raw, false);
        assert_eq!(out, "z");
    }

    #[test]
    fn message_display_strips_redacted_in_reasoning_text_field() {
        let m = StoredMessage {
            id: "x".into(),
            role: "assistant".into(),
            text: "visible".into(),
            reasoning_text: concat!(
                "<", "redacted", "_", "thinking", ">", "r", "</", "redacted", "_", "thinking", ">",
            )
            .into(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            created_at: 0,
        };
        let out = message_text_for_display_ex(&m, Locale::ZhHans, true);
        assert_eq!(out, "visible");
        assert!(!out.contains('r'));
    }

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
            created_at: 0,
        };
        let out = message_text_for_display_ex(&m, Locale::ZhHans, true);
        assert!(!out.contains("用户问"), "preamble should not leak: {out}");
        assert!(
            !out.contains("agent_reply_plan"),
            "json should not leak: {out}"
        );
    }
}
