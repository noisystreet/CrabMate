//! `on_done` / `on_error` / `on_workspace_changed` 闭包工厂。

use std::rc::Rc;

use leptos::prelude::*;

use crate::app::chat::session_hydrate::bump_session_hydrate_nonce;
use crate::app::chat::session_storage::persist_chat_sessions_at_stream_end;
use crate::i18n;
use crate::stream_text_overlay::{
    stream_overlay_clear_answer_for_message, stream_overlay_take_into_stored_message,
};

use super::super::super::context::ChatStreamCallbackCtx;
use super::super::super::per_stream_accum::PerStreamAccum;
use super::super::super::shell_abort::{clear_abort_slot, user_cancelled_flag};
use super::super::super::stream_control_reducer::StreamControlEvent;
use super::super::done_session::apply_stream_done_to_loading_assistant;
use super::super::error_session::apply_stream_error_on_messages;
use super::super::helpers::build_stream_error_with_suggestion;
use super::super::turn_layout::TurnLayout;

pub(in super::super) fn chat_stream_on_done_builder(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    accum: Rc<PerStreamAccum>,
) -> Rc<dyn Fn()> {
    Rc::new(move || {
        if user_cancelled_flag(&stream_ctx.shell) {
            stream_ctx.scratch.clear_followup_pending();
            clear_abort_slot(&stream_ctx.shell);
            stream_ctx.scratch.apply_stream_control_event(
                &stream_ctx.shell.stream,
                StreamControlEvent::StreamUserAbort,
            );
            return;
        }
        if stream_ctx.is_stale() {
            return;
        }
        // pending 若一直保留到 on_done，说明该 answer_phase 后没有正文 delta：
        // 普通 delta 会在 apply_chat_stream_text_delta 入口先消费 pending 并轮换。
        // 此处只清除 pending，由统一收尾投影现有 overlay；禁止创建无正文的新气泡。
        stream_ctx.scratch.clear_followup_pending();
        let turn = accum.summarize_for_stream_done();
        let loc = stream_ctx.locale.get_untracked();
        let mid = stream_ctx.scratch.clone_assistant_id();
        stream_ctx
            .scratch
            .finalize_turn_projection_before_stream_done(stream_ctx.as_ref());
        stream_ctx.update_bound_session(|s| {
            let sid = stream_ctx.bound_stream_session_id.as_str();
            // Phase C：`finalize`/`drain` 已投影终答并清 overlay；此处只清 loading 句柄，禁止 take 进壳升格。
            stream_overlay_clear_answer_for_message(
                stream_ctx.chat.stream_text_overlay,
                sid,
                mid.as_str(),
                Some(stream_ctx.chat.stream_overlay_revision),
            );
            if let Some(idx) = s.messages.iter().position(|m| m.id == mid.as_str()) {
                s.messages[idx].text.clear();
            }
            TurnLayout::dedupe_loading_tail_against_final_answer_row(&mut s.messages, mid.as_str());
            TurnLayout::dedupe_loading_tail_against_commentary_rows(&mut s.messages, mid.as_str());
            apply_stream_done_to_loading_assistant(
                &mut s.messages,
                mid.as_str(),
                &turn,
                stream_ctx
                    .scratch
                    .current_output_lane()
                    .in_answer_body_lane(),
                loc,
            );
        });
        // 将 `turn-final-answer` 脱钩为普通 assistant 行，
        // 防止下一轮 `sync_turn_projection` 覆盖时挤掉已显示的旧文本。
        TurnLayout::detach_final_answer_projection(stream_ctx.as_ref());
        stream_ctx.chat.clear_stream_text_overlay();
        persist_chat_sessions_at_stream_end(stream_ctx.chat, loc);
        stream_ctx
            .shell
            .stream
            .apply_release_turn_and_stream_run(stream_ctx.attach_generation);
        clear_abort_slot(&stream_ctx.shell);
        stream_ctx
            .scratch
            .apply_stream_control_event(&stream_ctx.shell.stream, StreamControlEvent::StreamDone);
        bump_session_hydrate_nonce(stream_ctx.chat);
    })
}

pub(in super::super) fn chat_stream_on_error_builder(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
) -> Rc<dyn Fn(String)> {
    Rc::new(move |msg: String| {
        if user_cancelled_flag(&stream_ctx.shell) {
            clear_abort_slot(&stream_ctx.shell);
            stream_ctx.scratch.apply_stream_control_event(
                &stream_ctx.shell.stream,
                StreamControlEvent::StreamUserAbort,
            );
            return;
        }
        if stream_ctx.is_stale() {
            return;
        }
        stream_ctx.chat.clear_stream_resume_handles();
        let mid = stream_ctx.scratch.clone_assistant_id();
        let loc = stream_ctx.locale.get_untracked();
        let friendly = build_stream_error_with_suggestion(&msg, loc);
        stream_ctx.update_bound_session(|s| {
            let sid = stream_ctx.bound_stream_session_id.as_str();
            if let Some(idx) = s.messages.iter().position(|m| m.id == mid.as_str()) {
                stream_overlay_take_into_stored_message(
                    stream_ctx.chat.stream_text_overlay,
                    sid,
                    mid.as_str(),
                    &mut s.messages[idx],
                );
            }
            apply_stream_error_on_messages(&mut s.messages, mid.as_str(), friendly, loc);
        });
        stream_ctx
            .shell
            .stream
            .apply_release_turn_and_stream_run(stream_ctx.attach_generation);
        stream_ctx.shell.stream.status_err.set(Some(
            i18n::chat_failed_banner(stream_ctx.locale.get_untracked()).to_string(),
        ));
        clear_abort_slot(&stream_ctx.shell);
        stream_ctx
            .scratch
            .apply_stream_control_event(&stream_ctx.shell.stream, StreamControlEvent::StreamError);
        bump_session_hydrate_nonce(stream_ctx.chat);
    })
}

pub(in super::super) fn chat_stream_on_ws_builder(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
) -> Rc<dyn Fn()> {
    Rc::new(move || {
        if stream_ctx.is_stale() {
            return;
        }
        (stream_ctx.shell.refresh_workspace)();
        stream_ctx
            .shell
            .ide
            .ide_sync_disk_nonce
            .update(|n| *n = n.saturating_add(1));
        if stream_ctx.shell.modal.changelist_modal_open.get_untracked() {
            stream_ctx
                .shell
                .modal
                .changelist_fetch_nonce
                .update(|x| *x = x.wrapping_add(1));
        }
    })
}
