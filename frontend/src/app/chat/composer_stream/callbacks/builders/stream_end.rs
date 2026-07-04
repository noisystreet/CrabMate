//! `on_done` / `on_error` / `on_workspace_changed` 闭包工厂。

use std::rc::Rc;

use leptos::prelude::*;

use crate::app::chat::session_hydrate::bump_session_hydrate_nonce;
use crate::i18n;
use crate::stream_text_overlay::stream_overlay_take_into_stored_message;

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
            stream_ctx
                .scratch
                .apply_stream_control_event(StreamControlEvent::StreamUserAbort);
            return;
        }
        if stream_ctx.is_stale() {
            return;
        }
        // 第二次 `assistant_answer_phase` 后若再无正文增量，须在此补做轮换并清零计数器；
        // 否则 `answer_delta_chars` 仍为上一轮时间轴累计，易误判「有输出却无正文」。
        if stream_ctx.scratch.take_followup_rotation_pending() {
            TurnLayout::rotate_followup_model_round(stream_ctx.as_ref());
            accum.clear_answer_delta_chars();
        }
        let turn = accum.summarize_for_stream_done();
        let loc = stream_ctx.locale.get_untracked();
        let mid = stream_ctx.scratch.clone_assistant_id();
        TurnLayout::dedupe_redundant_loading_tail(stream_ctx.as_ref());
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
            TurnLayout::dedupe_assistant_duplicates_in_messages(&mut s.messages);
        });
        stream_ctx.chat.clear_stream_text_overlay();
        stream_ctx
            .shell
            .stream
            .apply_release_turn_and_stream_run(stream_ctx.attach_generation);
        clear_abort_slot(&stream_ctx.shell);
        stream_ctx
            .scratch
            .apply_stream_control_event(StreamControlEvent::StreamDone);
        bump_session_hydrate_nonce(stream_ctx.chat);
    })
}

pub(in super::super) fn chat_stream_on_error_builder(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
) -> Rc<dyn Fn(String)> {
    Rc::new(move |msg: String| {
        if user_cancelled_flag(&stream_ctx.shell) {
            clear_abort_slot(&stream_ctx.shell);
            stream_ctx
                .scratch
                .apply_stream_control_event(StreamControlEvent::StreamUserAbort);
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
            .apply_stream_control_event(StreamControlEvent::StreamError);
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
