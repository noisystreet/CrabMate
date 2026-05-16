//! [`super::handles::WireComposerStreamsArgs`] → [`super::handles::ChatComposerWires`] 的接线实现，降低 `composer` 单文件圈复杂度。
//!
//! 子模块拆分：**`helpers`** 为发送路径纯函数；**`follow_up`** 单独承载待发流式队列的 `Effect`，便于审阅「单数据流」边界。

mod follow_up;
mod helpers;

use std::rc::Rc;
use std::sync::Arc;

use leptos::prelude::*;

use self::follow_up::{StreamFollowUpWiring, wire_stream_follow_up_effect};
use self::helpers::{
    begin_stream_shell_turn, push_user_and_loading_assistant, user_line_and_clarify_from_shell,
};
use super::composer_follow_up::ComposerStreamFollowUp;
use super::composer_stream::{ComposerStreamHandles, make_attach_chat_stream};
use super::handles::{
    ChatComposerWires, WireComposerStreamsArgs, WireComposerStreamsSessionSlice,
    WireComposerStreamsStreamSlice,
};
use super::stream_follow_up_gates::compose_user_send_allowed;
use super::stream_user_abort::apply_user_abort_of_inflight_stream;
use crate::session_ops::{flush_active_composer_draft, make_message_id};
use crate::session_sync::SessionSyncState;
use crate::storage::{ChatSession, DEFAULT_CHAT_SESSION_TITLE, make_session_id};

pub(crate) fn wire_chat_composer_streams(args: WireComposerStreamsArgs) -> ChatComposerWires {
    let WireComposerStreamsArgs { session, stream } = args;
    let WireComposerStreamsSessionSlice {
        initialized,
        chat,
        locale,
        draft,
        selected_agent_role,
    } = session;
    let WireComposerStreamsStreamSlice {
        stream_shell,
        stream_turn_busy_ui,
        auto_scroll_chat,
        pending_images,
    } = stream;

    let stream_shell_for_attach = stream_shell.clone();
    let attach_chat_stream = make_attach_chat_stream(ComposerStreamHandles {
        chat,
        locale,
        selected_agent_role,
        shell: stream_shell_for_attach,
    });

    let run_send_message: Arc<dyn Fn() + Send + Sync> = Arc::new({
        let chat = chat;
        let attach = Arc::clone(&attach_chat_stream);
        let auto_scroll_chat = auto_scroll_chat;
        let shell = stream_shell.clone();
        let locale_sig = locale;
        move || {
            let text = draft.get_untracked().trim().to_string();
            let imgs = pending_images.get();
            let loc = locale_sig.get();
            let Some((user_line, clarify_json)) =
                user_line_and_clarify_from_shell(&shell, &text, loc)
            else {
                return;
            };
            if !compose_user_send_allowed(
                initialized.get(),
                stream_turn_busy_ui.get(),
                user_line.is_empty(),
                imgs.is_empty(),
                clarify_json.is_none(),
            ) {
                return;
            }
            auto_scroll_chat.set(true);
            let uid = make_message_id();
            let asst_id = make_message_id();
            let imgs_send = imgs.clone();
            push_user_and_loading_assistant(
                chat,
                user_line.clone(),
                imgs_send.clone(),
                uid,
                asst_id.clone(),
            );
            draft.set(String::new());
            pending_images.set(Vec::new());
            begin_stream_shell_turn(&shell);
            attach(user_line, imgs_send, asst_id, clarify_json);
        }
    });

    let stream_follow_up = RwSignal::new(ComposerStreamFollowUp::Idle);

    wire_stream_follow_up_effect(StreamFollowUpWiring {
        initialized,
        chat,
        attach_chat_stream: Arc::clone(&attach_chat_stream),
        auto_scroll_chat,
        shell: stream_shell.clone(),
        stream_follow_up,
        stream_turn_busy_ui,
    });

    let cancel_stream: Arc<dyn Fn() + Send + Sync> = Arc::new({
        let chat = chat;
        let shell = stream_shell.clone();
        let locale = locale;
        move || {
            let loc = locale.get_untracked();
            let _ = apply_user_abort_of_inflight_stream(chat, &shell, loc);
        }
    });

    let new_session: Rc<dyn Fn()> = Rc::new({
        let chat = chat;
        move || {
            flush_active_composer_draft(chat.sessions, chat.active_id, draft);
            let prev_id = chat.active_id.get_untracked();
            let inherited_ws = chat.sessions.with_untracked(|list| {
                list.iter()
                    .find(|s| s.id == prev_id)
                    .and_then(|s| s.workspace_root.clone())
            });
            let now = js_sys::Date::now() as i64;
            let s = ChatSession {
                id: make_session_id(),
                title: DEFAULT_CHAT_SESSION_TITLE.to_string(),
                draft: String::new(),
                messages: vec![],
                updated_at: now,
                pinned: false,
                starred: false,
                server_conversation_id: None,
                server_revision: None,
                workspace_root: inherited_ws,
            };
            let id = s.id.clone();
            chat.update_sessions_composer(|list| {
                list.insert(0, s);
            });
            chat.active_id.set(id);
            draft.set(String::new());
            chat.session_sync.set(SessionSyncState::local_only());
        }
    });

    ChatComposerWires {
        stream_follow_up,
        run_send_message,
        cancel_stream,
        new_session,
    }
}
