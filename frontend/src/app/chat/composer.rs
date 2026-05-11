//! 输入区与流式对话：草稿缓冲、发送 / 停止、重试 / 截断再生、新会话。
//!
//! `/chat/stream` 的 SSE 回调装配见 [`super::composer_stream`]；流式接线实现见 [`super::composer_wires`]。

use leptos::html::Textarea;
use leptos::prelude::*;
use leptos::task::spawn_local;

use gloo_timers::future::TimeoutFuture;
use std::collections::HashSet;
use web_sys::HtmlTextAreaElement;

use crate::chat_session_state::ChatSessionSignals;
use crate::clarification_form::PendingClarificationForm;
use crate::session_sync::SessionSyncState;
use crate::storage::ChatSession;

use super::composer_mirror::composer_workspace_at_refs_html;

pub(crate) use super::composer_wires::wire_chat_composer_streams;

/// 用单次 `sessions` 快照刷新壳层状态（草稿、`session_sync`、流式 job 重置等）。
///
/// `sessions_snapshot` **必须**由调用方通过 [`RwSignal::get_untracked`]（或等价「不订阅」快照）提供；
/// 若在响应式 `Effect` 内改为 `sessions.with`/`get`，effect 会订阅每条流式消息写入并反复执行本逻辑，
/// 覆盖合成器缓冲。
fn apply_shell_after_active_session_changed(
    chat: &ChatSessionSignals,
    draft: RwSignal<String>,
    pending_images: RwSignal<Vec<String>>,
    pending_clarification: RwSignal<Option<PendingClarificationForm>>,
    collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    tool_detail_expanded_ids: RwSignal<HashSet<String>>,
    sessions_snapshot: &[ChatSession],
    active_id: &str,
) {
    let active = sessions_snapshot.iter().find(|s| s.id == active_id);
    let d = active.map(|s| s.draft.clone()).unwrap_or_default();
    draft.set(d);
    pending_images.set(Vec::new());
    pending_clarification.set(None);
    let st = active.map(|s| {
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
    chat.clear_stream_resume_handles();
    collapsed_long_assistant_ids.set(Vec::new());
    tool_detail_expanded_ids.set(HashSet::new());
}

/// 切换会话时重置会话级 UI 状态并加载该会话草稿。
///
/// **依赖**：`Effect` 仅追踪 `active_id` 与 `initialized`；会话列表通过 `get_untracked` 传入
/// [`apply_shell_after_active_session_changed`]（见该函数说明）。
pub(crate) fn wire_session_switch_clears_chat_state(
    initialized: RwSignal<bool>,
    chat: ChatSessionSignals,
    draft: RwSignal<String>,
    pending_images: RwSignal<Vec<String>>,
    pending_clarification: RwSignal<Option<PendingClarificationForm>>,
    collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    tool_detail_expanded_ids: RwSignal<HashSet<String>>,
) {
    Effect::new(move |_| {
        let id = chat.active_id.get();
        if !initialized.get() {
            return;
        }
        let list = chat.sessions.get_untracked();
        apply_shell_after_active_session_changed(
            &chat,
            draft,
            pending_images,
            pending_clarification,
            collapsed_long_assistant_ids,
            tool_detail_expanded_ids,
            list.as_slice(),
            id.as_str(),
        );
    });
}

/// `draft` 变更时同步 `@引用` 镜像与 textarea。
///
/// 用户输入走 `on:input` → `draft`，浏览器会先更新 textarea 的 `value`；同一轮里 `Effect` 若立刻
/// `set_value` 会与尚未提交的 DOM 产生竞态。此处先 `0ms` 再 `1ms` 延迟比对，多数情况可跳过 `set_value`。
///
/// **重要**：每次 `draft` 变化都会 `spawn_local` 新任务；快速输入时旧任务若仍用当时的 `d_for_dom`
/// 去 `set_value`，会截断正文并重置选区（光标卡死）。故在每次 `await` 之后、`set_value` 之前须用
/// `draft.get_untracked() == d_for_dom` 判定任务是否仍代表「当前」草稿，否则直接放弃。
fn sync_textarea_dom_from_draft_if_still_stale(ta: &HtmlTextAreaElement, new_val: &str) {
    if ta.value() == new_val {
        return;
    }
    let old_v = ta.value();
    let old_u16 = old_v.encode_utf16().count() as u32;
    let new_u16 = new_val.encode_utf16().count() as u32;
    let raw_start = ta.selection_start().ok().flatten().unwrap_or(old_u16);
    let raw_end = ta.selection_end().ok().flatten().unwrap_or(raw_start);
    let start = raw_start.min(old_u16);
    let end = raw_end.min(old_u16).max(start);
    ta.set_value(new_val);
    // `"".starts_with("")` 为真；空串作「旧 DOM」时勿走前缀分支，否则选区映射易错（甚至表现为跳到行首）。
    let (s, e) = if !old_v.is_empty() && new_val.starts_with(&old_v) {
        let map = |p: u32| {
            let p = p.min(old_u16);
            if p == old_u16 { new_u16 } else { p }
        };
        let s = map(start);
        (s, map(end).max(s))
    } else {
        (new_u16, new_u16)
    };
    let s = s.min(new_u16);
    let e = e.min(new_u16).max(s);
    let _ = ta.set_selection_range(s, e);
}

pub(crate) fn wire_draft_sync_to_mirror_and_textarea(
    draft: RwSignal<String>,
    composer_input_ref: NodeRef<Textarea>,
    composer_mirror_html: RwSignal<String>,
    composer_mirror_scroll_top: RwSignal<f64>,
) {
    Effect::new({
        let composer_input_ref = composer_input_ref.clone();
        let draft_for_stale = draft;
        move |_| {
            let d = draft_for_stale.get();
            composer_mirror_html.set(composer_workspace_at_refs_html(&d));
            composer_mirror_scroll_top.set(0.0);
            let d_for_dom = d.clone();
            let cref = composer_input_ref.clone();
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                if draft_for_stale.get_untracked() != d_for_dom {
                    return;
                }
                let Some(el) = cref.get_untracked() else {
                    return;
                };
                if el.value() == d_for_dom {
                    return;
                }
                TimeoutFuture::new(1).await;
                if draft_for_stale.get_untracked() != d_for_dom {
                    return;
                }
                let Some(el) = cref.get_untracked() else {
                    return;
                };
                if el.value() == d_for_dom {
                    return;
                }
                sync_textarea_dom_from_draft_if_still_stale(&el, &d_for_dom);
            });
        }
    });
}
