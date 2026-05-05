//! 输入区与流式对话：草稿缓冲、发送 / 停止、重试 / 截断再生、新会话。
//!
//! `/chat/stream` 的 SSE 回调装配见 [`super::composer_stream`]；流式接线实现见 [`super::composer_wires`]。

use leptos::html::Textarea;
use leptos::prelude::*;
use leptos::task::spawn_local;

use gloo_timers::future::TimeoutFuture;

use crate::chat_session_state::ChatSessionSignals;
use crate::clarification_form::PendingClarificationForm;
use crate::session_sync::SessionSyncState;

use super::composer_mirror::composer_workspace_at_refs_html;

pub(crate) use super::composer_wires::wire_chat_composer_streams;

/// 切换会话时重置会话级 UI 状态并加载该会话草稿（勿订阅 `sessions`，避免流式更新覆盖缓冲）。
pub(crate) fn wire_session_switch_clears_chat_state(
    initialized: RwSignal<bool>,
    chat: ChatSessionSignals,
    draft: RwSignal<String>,
    pending_images: RwSignal<Vec<String>>,
    pending_clarification: RwSignal<Option<PendingClarificationForm>>,
    collapsed_long_assistant_ids: RwSignal<Vec<String>>,
) {
    Effect::new(move |_| {
        let id = chat.active_id.get();
        if !initialized.get() {
            return;
        }
        let list = chat.sessions.get_untracked();
        let d = list
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.draft.clone())
            .unwrap_or_default();
        draft.set(d);
        pending_images.set(Vec::new());
        pending_clarification.set(None);
        // 仅用上方 `list`（get_untracked）：勿再 `sessions.with`，否则 effect 会订阅流式
        // `sessions` 更新。
        let st = list.iter().find(|s| s.id == id).map(|s| {
            let mut st = SessionSyncState::local_only();
            if let Some(ref cid) = s.server_conversation_id {
                let t = cid.trim();
                if !t.is_empty() {
                    st.apply_stream_conversation_id(t.to_string());
                    if let Some(rev) = s.server_revision {
                        st.apply_saved_revision(rev);
                    }
                }
            }
            st
        });
        chat.session_sync
            .set(st.unwrap_or_else(SessionSyncState::local_only));
        chat.stream_job_id.set(None);
        chat.stream_last_event_seq.set(0);
        collapsed_long_assistant_ids.set(Vec::new());
    });
}

/// `draft` 变更时同步 `@引用` 镜像与 textarea（用户输入亦写入同一 `draft`，与 DOM 不等时再 `set_value`，避免误伤光标）。
pub(crate) fn wire_draft_sync_to_mirror_and_textarea(
    draft: RwSignal<String>,
    composer_input_ref: NodeRef<Textarea>,
    composer_mirror_html: RwSignal<String>,
    composer_mirror_scroll_top: RwSignal<f64>,
) {
    Effect::new({
        let composer_input_ref = composer_input_ref.clone();
        move |_| {
            let d = draft.get();
            composer_mirror_html.set(composer_workspace_at_refs_html(&d));
            composer_mirror_scroll_top.set(0.0);
            let d_for_dom = d.clone();
            let cref = composer_input_ref.clone();
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                if let Some(el) = cref.get_untracked() {
                    if el.value() != d_for_dom {
                        el.set_value(&d_for_dom);
                    }
                }
            });
        }
    });
}
