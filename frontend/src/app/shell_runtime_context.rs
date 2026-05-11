//! Leptos [`provide_context`]：向子树提供**可 `Copy`** 的聊天域句柄，减轻与 [`AppSignals`](super::app_signals::AppSignals) 同源的长 props 传递。
//!
//! 全量壳层仍用 [`super::app_shell_ctx::AppShellCtx`]（含 `Rc` / `StoredValue` 等）；此处仅收敛满足
//! `Send + Sync + 'static` 的聊天切片。典型消费方：**`sidebar_nav_view`**、**`session_list_modal_view`**、
//! **`status_bar_footer_view`**、**`SessionContextMenuLayer`**、**[`crate::app::chat::find_bar::ChatFindBar`]**（见各自模块）。
//!
//! 新增子树消费方优先 [`expect_chat_shell_ctx`]；单测或 Story 可在挂载前 [`provide_context`]，或改用
//! [`try_chat_shell_ctx`] 做缺失分支。

use leptos::prelude::*;

use crate::app::app_signals::{AppSignals, ChatComposerSignals};
use crate::chat_session_state::ChatSessionSignals;

/// 聊天域：会话 / 作曲器、界面语言与助手正文过滤（与 [`AppSignals`] 内对应字段同源）。
#[derive(Clone, Copy)]
pub struct ChatShellLeptosContext {
    pub chat: ChatSessionSignals,
    pub composer: ChatComposerSignals,
    pub locale: RwSignal<crate::i18n::Locale>,
    pub apply_assistant_display_filters: RwSignal<bool>,
}

impl ChatShellLeptosContext {
    #[must_use]
    pub fn from_app_signals(app: &AppSignals) -> Self {
        Self {
            chat: app.chat,
            composer: app.chat_composer,
            locale: app.shell_ui.locale,
            apply_assistant_display_filters: app.shell_ui.apply_assistant_display_filters,
        }
    }
}

/// 在 `App` 根等已 [`provide_context`] 的子树内调用；缺失时 panic（开发期即暴露接线错误）。
#[must_use]
pub fn expect_chat_shell_ctx() -> ChatShellLeptosContext {
    try_chat_shell_ctx()
        .expect("ChatShellLeptosContext: provide ChatShellLeptosContext in App root")
}

/// 与 [`expect_chat_shell_ctx`] 同源；无上下文时返回 `None`（便于单测 / 可选子树）。
#[must_use]
pub fn try_chat_shell_ctx() -> Option<ChatShellLeptosContext> {
    use_context::<ChatShellLeptosContext>()
}
