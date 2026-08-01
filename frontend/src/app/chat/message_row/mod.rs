//! 单条消息气泡与下方操作条（复制 / 重试 / 分支等）。
//!
//! 与 `POST /chat/branch`、本地截断再生相关的副作用见 [`super::message_row_actions`]。
//!
//! 主列当前默认终端流；本模块随气泡列表一并保留。
#![allow(dead_code)]

pub(crate) mod helpers;
mod non_assistant_body;
mod row;
mod row_extras;
mod views;

use std::collections::{HashMap, HashSet};

use super::composer_follow_up::ComposerStreamFollowUp;
use super::scroll_shell::ChatScrollShellSignals;
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
    pub scroll_shell: ChatScrollShellSignals,
    pub stream_turn_busy_ui: Memo<bool>,
    /// 当前活动会话尾部 loading 助手消息 id；仅该行显示打字点，避免每行 `sessions.with`。
    pub tail_loading_assistant_mid: Memo<Option<String>>,
    pub stream_follow_up: RwSignal<ComposerStreamFollowUp>,
    pub status_err: RwSignal<Option<String>>,
    pub locale: RwSignal<Locale>,
    pub markdown_render: RwSignal<bool>,
    pub apply_assistant_display_filters: RwSignal<bool>,
    /// 工具气泡详情抽屉展开状态（按消息 id，跨 `For` 重挂保留）。
    pub tool_detail_expanded_ids: RwSignal<HashSet<String>>,
    /// 预计算的每行 (loading, error) 状态映射，避免单行 `sessions.with` 全表扫描。
    pub row_state_map: Memo<HashMap<String, (bool, bool)>>,
}

pub(crate) use row::chat_message_row;
