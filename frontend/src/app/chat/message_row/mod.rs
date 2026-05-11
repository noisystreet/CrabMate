//! 单条消息气泡与下方操作条（复制 / 重试 / 分支等）。
//!
//! 与 `POST /chat/branch`、本地截断再生相关的副作用见 [`super::message_row_actions`]。

mod helpers;
mod non_assistant_body;
mod row;
mod row_extras;
mod views;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::storage::StoredMessage;

use leptos::prelude::*;

/// 聊天消息行视图所需信号与数据（缩短 [`chat_message_row`] 形参列表；勿命名为 `*Props`，与 Leptos 组件宏生成类型冲突）。
#[derive(Clone)]
pub(crate) struct ChatMessageRowSignals {
    pub msg_idx: usize,
    pub m: StoredMessage,
    pub chat: ChatSessionSignals,
    pub collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    pub chat_find_query: RwSignal<String>,
    pub chat_find_match_ids: RwSignal<Vec<String>>,
    pub chat_find_cursor: RwSignal<usize>,
    pub auto_scroll_chat: RwSignal<bool>,
    pub stream_turn_busy_ui: Memo<bool>,
    /// 当前活动会话尾部 loading 助手消息 id；仅该行显示打字点，避免每行 `sessions.with`。
    pub tail_loading_assistant_mid: Memo<Option<String>>,
    pub regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    pub retry_assistant_target: RwSignal<Option<String>>,
    pub status_err: RwSignal<Option<String>>,
    pub locale: RwSignal<Locale>,
    pub markdown_render: RwSignal<bool>,
    pub apply_assistant_display_filters: RwSignal<bool>,
}

pub(crate) use row::chat_message_row;

#[cfg(test)]
mod tests {
    use super::helpers::{
        build_subgoal_exec_banner_icon_key, build_subgoal_exec_banner_text,
        extract_hierarchical_goal_target, is_running_subgoal_phase,
    };
    use crate::i18n::{self, Locale};
    use crate::storage::{StoredMessage, StoredMessageState};

    fn subgoal_msg(text: &str) -> StoredMessage {
        StoredMessage {
            id: "m1".to_string(),
            role: "assistant".to_string(),
            text: text.to_string(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::HierarchicalSubgoal(
                "hierarchical-subgoal:goal_5".into(),
            )),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn extract_goal_target_from_subgoal_text() {
        let m = subgoal_msg("- 阶段：开始执行\n- 目标：创建 build 目录");
        let target = extract_hierarchical_goal_target(&m);
        assert_eq!(target.as_deref(), Some("创建 build 目录"));
    }

    #[test]
    fn build_exec_banner_for_started_phase() {
        let t = build_subgoal_exec_banner_text(
            Locale::ZhHans,
            Some("开始执行"),
            Some("创建 build 目录并运行 cmake"),
        );
        assert_eq!(t.as_deref(), Some("正在执行：创建 build 目录并运行 cmake…"));
    }

    #[test]
    fn build_exec_banner_for_fix_phase() {
        let t =
            build_subgoal_exec_banner_text(Locale::ZhHans, Some("修复"), Some("修正 CMake 路径"));
        assert_eq!(t.as_deref(), Some("正在修复：修正 CMake 路径…"));
    }

    #[test]
    fn build_exec_banner_icon_for_verify_phase() {
        let icon = build_subgoal_exec_banner_icon_key(Locale::ZhHans, Some("验证"));
        assert_eq!(icon, Some("verify"));
    }

    #[test]
    fn running_subgoal_phase_only_for_active_progress() {
        assert!(is_running_subgoal_phase(Locale::ZhHans, Some("修复")));
        assert!(!is_running_subgoal_phase(Locale::ZhHans, Some("完成")));
    }

    #[test]
    fn phase_key_is_locale_independent() {
        assert_eq!(
            i18n::hierarchical_subgoal_phase_key(Some("开始执行")),
            Some("run")
        );
        assert_eq!(
            i18n::hierarchical_subgoal_phase_key(Some("diagnose")),
            Some("diagnose")
        );
    }
}
