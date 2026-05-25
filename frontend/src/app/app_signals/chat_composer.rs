//! 聊天输入区、查找、滚底与镜像层等。

use std::collections::HashSet;

use leptos::html::Div;
use leptos::prelude::*;

#[derive(Clone, Copy)]
pub struct ChatComposerSignals {
    pub draft: RwSignal<String>,
    pub pending_images: RwSignal<Vec<String>>,
    /// 输入框镜像层 HTML（`@{工作区路径}` 高亮）；与草稿缓冲同源更新。
    pub composer_mirror_html: RwSignal<String>,
    pub composer_mirror_scroll_top: RwSignal<f64>,
    pub composer_input_ref: NodeRef<leptos::html::Textarea>,
    pub collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    /// 连续工具组中用户手动**收起**为仅显示最后一条的分组 head（message id）；默认空 = 全部展开。
    pub collapsed_tool_run_heads: RwSignal<HashSet<String>>,
    /// 工具气泡「详情抽屉」展开中的消息 id（与 `StoredMessage::id` 一致）；避免 `For` 重挂行时丢失本地 `RwSignal<bool>`。
    pub tool_detail_expanded_ids: RwSignal<HashSet<String>>,
    pub auto_scroll_chat: RwSignal<bool>,
    pub messages_scroll_from_effect: RwSignal<bool>,
    pub last_messages_scroll_top: RwSignal<i32>,
    /// 消息列 `scrollTop` / `clientHeight`（虚拟窗口与 prepend 后滚动补偿）。
    pub virtual_scroll_top: RwSignal<i32>,
    pub virtual_viewport_height: RwSignal<i32>,
    pub messages_scroller: NodeRef<Div>,
    pub timeline_panel_expanded: RwSignal<bool>,
    pub chat_find_query: RwSignal<String>,
    pub chat_find_match_ids: RwSignal<Vec<String>>,
    pub chat_find_cursor: RwSignal<usize>,
    pub chat_find_panel_open: RwSignal<bool>,
    pub focus_message_id_after_nav: RwSignal<Option<String>>,
}

impl ChatComposerSignals {
    pub fn new() -> Self {
        Self {
            draft: RwSignal::new(String::new()),
            pending_images: RwSignal::new(Vec::new()),
            composer_mirror_html: RwSignal::new(String::new()),
            composer_mirror_scroll_top: RwSignal::new(0.0),
            composer_input_ref: NodeRef::new(),
            collapsed_long_assistant_ids: RwSignal::new(Vec::new()),
            collapsed_tool_run_heads: RwSignal::new(HashSet::new()),
            tool_detail_expanded_ids: RwSignal::new(HashSet::new()),
            auto_scroll_chat: RwSignal::new(true),
            messages_scroll_from_effect: RwSignal::new(false),
            last_messages_scroll_top: RwSignal::new(0),
            virtual_scroll_top: RwSignal::new(0),
            virtual_viewport_height: RwSignal::new(600),
            messages_scroller: NodeRef::new(),
            timeline_panel_expanded: RwSignal::new(
                crate::app::chat::load_timeline_panel_expanded_default(),
            ),
            chat_find_query: RwSignal::new(String::new()),
            chat_find_match_ids: RwSignal::new(Vec::new()),
            chat_find_cursor: RwSignal::new(0),
            chat_find_panel_open: RwSignal::new(false),
            focus_message_id_after_nav: RwSignal::new(None),
        }
    }
}

impl Default for ChatComposerSignals {
    fn default() -> Self {
        Self::new()
    }
}
