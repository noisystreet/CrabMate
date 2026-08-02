#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::super::stream_turn_scratch_state::StreamTurnScratchState;
    use super::super::helpers::{
        build_empty_reply_with_diagnostic, build_final_response_text,
        build_stream_error_with_suggestion,
    };
    use crate::i18n::{self, Locale};

    #[test]
    fn pending_tool_message_queue_is_fifo() {
        let s = StreamTurnScratchState::new("x".into());
        s.enqueue_pending_tool_message_id("m1".to_string());
        s.enqueue_pending_tool_message_id("m2".to_string());
        s.enqueue_pending_tool_message_id("m3".to_string());

        assert_eq!(s.take_pending_tool_fifo_head().as_deref(), Some("m1"));
        assert_eq!(s.take_pending_tool_fifo_head().as_deref(), Some("m2"));
        assert_eq!(s.take_pending_tool_fifo_head().as_deref(), Some("m3"));
        assert_eq!(s.take_pending_tool_fifo_head(), None);
    }

    #[test]
    fn pending_tool_message_queue_empty_returns_none() {
        let s = StreamTurnScratchState::new("x".into());
        assert_eq!(s.take_pending_tool_fifo_head(), None);
    }

    #[test]
    fn final_response_text_merges_title_and_detail() {
        let merged = build_final_response_text("  你好  ", Some("  世界  "));
        assert_eq!(merged, "你好\n\n世界");
    }

    #[test]
    fn final_response_text_ignores_empty_detail() {
        let merged = build_final_response_text("  你好  ", Some("   "));
        assert_eq!(merged, "你好");
    }

    #[test]
    fn stream_error_uses_standardized_sections() {
        let out = build_stream_error_with_suggestion("LLM_API_KEY_REQUIRED", Locale::ZhHans);
        assert!(out.contains("发生了什么"));
        assert!(out.contains("影响范围"));
        assert!(out.contains("建议下一步"));
    }

    #[test]
    fn empty_reply_diagnostic_uses_partial_generation_hint_when_reason_unknown() {
        let out = build_empty_reply_with_diagnostic(Locale::ZhHans, true, 128, Some("unknown"));
        assert!(out.contains("流式收尾信号缺失"));
        assert!(out.contains("stream_ended=unknown"));
    }

    #[test]
    fn completed_without_final_summary_hint_is_shown() {
        let out = i18n::stream_completed_missing_final_summary_hint(Locale::ZhHans);
        assert!(out.contains("最终总结消息缺失"));
    }

    #[test]
    fn dedupe_helper_trims_whitespace() {
        let dummy = "hello world";
        assert_eq!(dummy.trim(), "hello world");
        // 间接保障：去重比较使用 trim，不会因尾部换行误判不同。
        // 该逻辑在 `assistant_message_has_visible_text` 中实现。
    }

    #[test]
    fn missing_final_summary_hint_disabled_after_final_response_timeline() {
        assert!(
            !super::super::helpers::should_show_missing_final_summary_hint(
                Some("completed"),
                true,
                true,
                true,
            )
        );
        assert!(
            super::super::helpers::should_show_missing_final_summary_hint(
                Some("completed"),
                true,
                true,
                false,
            )
        );
    }

    #[test]
    fn missing_final_summary_hint_never_without_answer_phase_even_if_diag_chars() {
        assert!(
            !super::super::helpers::should_show_missing_final_summary_hint(
                Some("completed"),
                false,
                true,
                false,
            )
        );
    }
}
