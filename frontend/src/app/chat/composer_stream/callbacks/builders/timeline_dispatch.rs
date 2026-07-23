//! `on_timeline_log` 与按 `kind` 分发的时间线旁路逻辑。

use std::rc::Rc;

use leptos::prelude::GetUntracked;

use crate::app::turn_lifecycle::TurnLifecycleEvent;
use crate::sse_dispatch::TimelineLogInfo;
use crate::stream_text_overlay::{
    stream_overlay_answer_for_message, stream_overlay_replace_answer_for_message,
};
use crate::timeline_scan::{
    timeline_state_intent_analysis_snapshot, timeline_state_local_snapshot,
};

use super::super::super::context::ChatStreamCallbackCtx;
use super::super::super::per_stream_accum::PerStreamAccum;
use super::super::super::turn_canonical::IngestFinalResponseOutcome;
use super::super::helpers::*;
use super::super::turn_layout::TurnLayout;

/// 收敛写入：`final_response` 阶段 2 起写入 overlay（不再写 canonical），并投影到现有 loading 尾泡，**不** push 新 assistant 行。
fn timeline_log_dispatch_final_response(
    stream_ctx: &ChatStreamCallbackCtx,
    accum: &PerStreamAccum,
    info: &TimelineLogInfo,
) {
    accum.set_saw_final_response_timeline(true);
    stream_ctx
        .shell
        .stream
        .dispatch_turn_lifecycle(TurnLifecycleEvent::TimelineModelFinal {
            attach_generation: stream_ctx.attach_generation,
        });
    let final_text = build_final_response_text(&info.title, info.detail.as_deref());
    if !final_text.is_empty() {
        let already_visible = assistant_message_has_visible_text(stream_ctx, &final_text)
            || streaming_assistant_tail_has_text(stream_ctx, &final_text);
        if !already_visible {
            let mid = stream_ctx.scratch.clone_assistant_id();
            let sid = stream_ctx.bound_stream_session_id.clone();
            let current_overlay = stream_overlay_answer_for_message(
                stream_ctx.chat.stream_text_overlay.get_untracked().as_ref(),
                sid.as_str(),
                mid.as_str(),
            );
            let outcome = stream_ctx
                .scratch
                .try_ingest_final_response_text(final_text.as_str(), current_overlay.as_deref());
            let wrote_overlay = matches!(outcome, IngestFinalResponseOutcome::WriteToOverlay(_));
            if let IngestFinalResponseOutcome::WriteToOverlay(ref text) = outcome {
                stream_overlay_replace_answer_for_message(
                    stream_ctx.chat.stream_text_overlay,
                    sid.as_str(),
                    mid.as_str(),
                    text.as_str(),
                    Some(stream_ctx.chat.stream_overlay_revision),
                );
                // overlay 已被本路径写入，避免 `sync_stream_preview` 用 canonical `final_answer`
                // （阶段 2 后该路径不再写 canonical）replace 掉 overlay。
                stream_ctx.chat.set_stream_overlay_display_mid(mid.as_str());
            }
            if outcome.consumed() {
                stream_ctx.scratch.open_post_tool_final_answer_gate();
                stream_ctx.scratch.sync_turn_projection(stream_ctx);
                if !wrote_overlay {
                    // `Consumed`（未写 overlay）：canonical 未变，sync 是 no-op 安全；
                    // `WriteToOverlay` 已写 overlay，跳过避免清空。
                    stream_ctx.scratch.sync_stream_preview(stream_ctx);
                }
                accum.add_answer_delta_chars(final_text.chars().count());
            }
        }
        // 当 overlay 已有流式正文（already_visible=true）时，final_response 提前 finalize
        // 会消费 overlay，导致后续 on_done 时 sync_turn_projection 无法读取 overlay 创建
        // FINAL_ANSWER_ROW。此时延迟到 on_done 处理（finalize_turn_projection_before_stream_done）。
        // 仅在 final_response 写入了新 overlay（already_visible=false 且 outcome.consumed()）
        // 或 post-tool 延迟场景才跳过立即 finalize。
        let defer =
            already_visible || TurnLayout::should_defer_finalize_on_final_response(stream_ctx);
        if !defer {
            TurnLayout::finalize_loading_segment(stream_ctx);
        }
    } else {
        // 补偿收尾可能带空 final_response；若不撤 loading，on_done 会误报「未收到正文片段」。
        TurnLayout::remove_loading_placeholder_or_rotate(stream_ctx);
    }
}

fn timeline_log_dispatch_intent_analysis(
    stream_ctx: &ChatStreamCallbackCtx,
    _accum: &PerStreamAccum,
    info: &TimelineLogInfo,
) {
    let intent_text = build_intent_analysis_main_bubble_text(&info.title, info.detail.as_deref());
    if intent_text.is_empty() {
        return;
    }
    let state = Some(timeline_state_intent_analysis_snapshot());
    push_assistant_timeline_bubble(stream_ctx, intent_text.clone(), state);
}

fn timeline_log_dispatch_hierarchical_plan(
    stream_ctx: &ChatStreamCallbackCtx,
    accum: &PerStreamAccum,
    info: &TimelineLogInfo,
) {
    let plan_text = build_hierarchical_plan_main_bubble_text(&info.title, info.detail.as_deref());
    if plan_text.is_empty() {
        return;
    }
    let state = Some(timeline_state_local_snapshot());
    push_assistant_timeline_bubble(stream_ctx, plan_text.clone(), state);
    accum.add_answer_delta_chars(plan_text.chars().count());
}

fn timeline_log_dispatch_hierarchical_subgoal(
    stream_ctx: &ChatStreamCallbackCtx,
    accum: &PerStreamAccum,
    info: &TimelineLogInfo,
) {
    let text = build_hierarchical_subgoal_main_bubble_text(&info.title, info.detail.as_deref());
    if text.is_empty() {
        return;
    }
    accum.set_current_subgoal_marker(extract_subgoal_marker_from_title(&info.title));
    upsert_hierarchical_subgoal_bubble(stream_ctx, text.clone(), &info.title);
    accum.add_answer_delta_chars(text.chars().count());
}

fn timeline_log_dispatch_default_body(stream_ctx: &ChatStreamCallbackCtx, info: &TimelineLogInfo) {
    let mut body = info.title.trim().to_string();
    if let Some(detail) = info.detail.as_deref().map(str::trim)
        && !detail.is_empty()
    {
        body.push('\n');
        body.push_str(detail);
    }
    if body.is_empty() {
        return;
    }
    let state = Some(timeline_state_local_snapshot());
    push_assistant_timeline_bubble(stream_ctx, body, state);
}

fn timeline_log_dispatch_body(
    stream_ctx: &ChatStreamCallbackCtx,
    accum: &PerStreamAccum,
    info: TimelineLogInfo,
) {
    web_sys::console::log_1(&format!("[TL] kind={} title={}", info.kind, info.title).into());
    match info.kind.as_str() {
        "final_response" => timeline_log_dispatch_final_response(stream_ctx, accum, &info),
        "intent_analysis" => timeline_log_dispatch_intent_analysis(stream_ctx, accum, &info),
        "hierarchical_plan" => timeline_log_dispatch_hierarchical_plan(stream_ctx, accum, &info),
        "hierarchical_subgoal" | "hierarchical_subgoal_started" => {
            timeline_log_dispatch_hierarchical_subgoal(stream_ctx, accum, &info);
        }
        "planner_tool_call_rejected" | "orchestration_route" => {}
        "tool_step_started" | "tool_step_finished" => {}
        _ => timeline_log_dispatch_default_body(stream_ctx, &info),
    }
}

pub(in super::super) fn make_on_timeline_log(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    accum: Rc<PerStreamAccum>,
) -> Rc<dyn Fn(TimelineLogInfo)> {
    Rc::new(move |info: TimelineLogInfo| {
        if stream_ctx.is_stale() {
            return;
        }
        timeline_log_dispatch_body(&stream_ctx, accum.as_ref(), info);
    })
}

#[cfg(test)]
mod tests {
    use crate::app::chat::composer_stream::callbacks::TurnLayout;

    #[test]
    fn final_response_tail_match_defers_finalize_in_post_tool_phase() {
        assert!(!TurnLayout::should_finalize_loading_when_tail_matches_final_response(true));
    }

    #[test]
    fn final_response_tail_match_finalizes_when_not_post_tool() {
        assert!(TurnLayout::should_finalize_loading_when_tail_matches_final_response(false));
    }
}
