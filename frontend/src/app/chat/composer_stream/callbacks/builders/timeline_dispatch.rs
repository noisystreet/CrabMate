//! `on_timeline_log` 与按 `kind` 分发的时间线旁路逻辑。

use std::rc::Rc;

use crate::app::stream_shell_busy::StreamShellBusyOp;
use crate::message_format::staged_timeline_system_message_body;
use crate::sse_dispatch::TimelineLogInfo;
use crate::timeline_scan::{
    timeline_state_intent_analysis_snapshot, timeline_state_local_snapshot,
};

use super::super::super::context::ChatStreamCallbackCtx;
use super::super::super::per_stream_accum::PerStreamAccum;
use super::super::helpers::*;
use super::super::turn_layout::TurnLayout;

/// 收敛写入：`final_response` 只更新 canonical 并投影到现有 loading 尾泡，**不** push 新 assistant 行。
fn timeline_log_dispatch_final_response(
    stream_ctx: &ChatStreamCallbackCtx,
    accum: &PerStreamAccum,
    info: &TimelineLogInfo,
) {
    accum.set_saw_final_response_timeline(true);
    stream_ctx
        .shell
        .stream
        .apply_busy_op(StreamShellBusyOp::ReleaseStreamingStatusAfterTimelineFinal);
    let final_text = build_final_response_text(&info.title, info.detail.as_deref());
    if !final_text.is_empty() {
        let already_visible = assistant_message_has_visible_text(stream_ctx, &final_text)
            || streaming_assistant_tail_has_text(stream_ctx, &final_text);
        if !already_visible
            && stream_ctx
                .scratch
                .try_ingest_final_response_text(final_text.as_str())
        {
            stream_ctx.scratch.sync_stream_preview(stream_ctx);
            accum.add_answer_delta_chars(final_text.chars().count());
        }
        if !TurnLayout::should_defer_finalize_on_final_response(stream_ctx) {
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
    push_assistant_timeline_bubble(
        stream_ctx,
        staged_timeline_system_message_body(&body),
        state,
    );
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
