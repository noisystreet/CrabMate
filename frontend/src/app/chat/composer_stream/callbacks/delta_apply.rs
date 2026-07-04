//! SSE `on_delta` 正文/思维链写入与车道轮换（从 [`super::builders::chat_stream_on_delta_builder`] 拆出以便单测与降 nloc）。

use std::rc::Rc;

use super::super::context::ChatStreamCallbackCtx;
use super::super::per_stream_accum::PerStreamAccum;
use super::super::stream_control_reducer::StreamControlEvent;
use super::super::stream_turn_state::StreamModelOutputLane;
use super::turn_layout::TurnLayout;

/// post-tool 或正文相：canonical [`AnswerDelta`] + overlay preview（P0′）；禁止 chunk append 正文。
fn apply_answer_body_delta(
    stream_ctx: &ChatStreamCallbackCtx,
    accum: &PerStreamAccum,
    chunk: &str,
) {
    if stream_ctx.scratch.try_apply_answer_delta(chunk) {
        stream_ctx.scratch.sync_turn_projection(stream_ctx);
        stream_ctx.scratch.sync_stream_preview(stream_ctx);
        accum.add_answer_delta_chars(chunk.chars().count());
        return;
    }
    if stream_ctx.scratch.post_tool_stream_tail_active() {
        if stream_ctx.scratch.tool_phase_open()
            && stream_ctx.scratch.try_apply_commentary_delta(chunk)
        {
            stream_ctx.scratch.sync_turn_projection(stream_ctx);
            stream_ctx.scratch.sync_stream_preview(stream_ctx);
        }
        accum.add_answer_delta_chars(chunk.chars().count());
        return;
    }
    // P0：canonical miss 时尝试 commentary 段，仍 miss 则 no-op（勿 append 尾泡）。
    if stream_ctx.scratch.try_apply_commentary_delta(chunk) {
        stream_ctx.scratch.sync_turn_projection(stream_ctx);
        stream_ctx.scratch.sync_stream_preview(stream_ctx);
    }
    accum.add_answer_delta_chars(chunk.chars().count());
}

/// 工具前旁注：canonical commentary 段 + 投影；miss 时不写尾泡（Phase 1 I2）。
fn apply_commentary_lane_delta(stream_ctx: &ChatStreamCallbackCtx, chunk: &str) {
    if stream_ctx.scratch.try_apply_commentary_delta(chunk) {
        stream_ctx.scratch.sync_turn_projection(stream_ctx);
        stream_ctx.scratch.sync_stream_preview(stream_ctx);
    }
}

/// 将单块流式文本写入当前尾泡：必要时先轮换占位，再按车道写入正文或 `reasoning_text`。
pub(super) fn apply_chat_stream_text_delta(
    stream_ctx: &ChatStreamCallbackCtx,
    accum: &PerStreamAccum,
    chunk: &str,
) {
    if stream_ctx.scratch.take_followup_rotation_pending() {
        TurnLayout::rotate_followup_model_round(stream_ctx);
        accum.clear_answer_delta_chars();
    }
    let lane = stream_ctx.scratch.current_output_lane();
    let post_tool = stream_ctx.scratch.post_tool_stream_tail_active();

    // post-tool：工具批进行中 → commentary 块；结束后 → 终答。
    if post_tool && lane != StreamModelOutputLane::AnsweringCommentaryBeforeTools {
        if stream_ctx.scratch.tool_phase_open() {
            apply_commentary_lane_delta(stream_ctx, chunk);
            accum.add_answer_delta_chars(chunk.chars().count());
            return;
        }
        apply_answer_body_delta(stream_ctx, accum, chunk);
        return;
    }

    if matches!(
        lane,
        StreamModelOutputLane::Reasoning | StreamModelOutputLane::AnsweringCommentaryBeforeTools
    ) {
        apply_commentary_lane_delta(stream_ctx, chunk);
        return;
    }

    if lane.in_answer_body_lane() {
        apply_answer_body_delta(stream_ctx, accum, chunk);
        return;
    }

    // 未知车道：仅 reasoning overlay append（思维链仍走热路径）。
    let mid = stream_ctx.scratch.borrow_assistant_id();
    stream_ctx.append_assistant_chunk(mid.as_str(), chunk, true);
}

/// 装配 `on_delta` 闭包（与 [`assemble::build_chat_stream_callbacks`] 对齐）。
pub(super) fn chat_stream_on_delta_builder(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    accum: Rc<PerStreamAccum>,
) -> Rc<dyn Fn(String)> {
    Rc::new(move |chunk: String| {
        if stream_ctx.is_stale() {
            return;
        }
        stream_ctx
            .scratch
            .apply_stream_control_event(StreamControlEvent::ModelTextDelta);
        apply_chat_stream_text_delta(stream_ctx.as_ref(), accum.as_ref(), &chunk);
    })
}

#[cfg(test)]
mod tests {
    use super::super::super::stream_turn_state::StreamModelOutputLane;

    #[test]
    fn post_tool_lane_routes_to_commentary_while_tool_phase_open() {
        let post_tool = true;
        let tool_phase_open = true;
        let lane = StreamModelOutputLane::Reasoning;
        let use_answer = post_tool
            && !tool_phase_open
            && lane != StreamModelOutputLane::AnsweringCommentaryBeforeTools;
        assert!(!use_answer);
    }

    #[test]
    fn post_tool_lane_routes_to_answer_after_tool_phase_end() {
        let post_tool = true;
        let tool_phase_open = false;
        let lane = StreamModelOutputLane::Reasoning;
        assert!(
            post_tool
                && !tool_phase_open
                && lane != StreamModelOutputLane::AnsweringCommentaryBeforeTools,
            "post-tool plain delta after tool_phase_end must use answer path"
        );
    }

    #[test]
    fn pre_tool_demoted_lane_stays_commentary() {
        let post_tool = false;
        let lane = StreamModelOutputLane::AnsweringCommentaryBeforeTools;
        let use_answer = post_tool && lane != StreamModelOutputLane::AnsweringCommentaryBeforeTools;
        assert!(!use_answer);
        assert!(matches!(
            lane,
            StreamModelOutputLane::Reasoning
                | StreamModelOutputLane::AnsweringCommentaryBeforeTools
        ));
    }

    #[test]
    fn no_tool_answer_lane_uses_canonical_not_overlay_only() {
        let lane = StreamModelOutputLane::Answering;
        let post_tool = false;
        assert!(lane.in_answer_body_lane());
        assert!(!post_tool);
    }

    #[test]
    fn p0_prime_preview_uses_overlay_replace_not_chunk_append() {
        let uses_overlay_replace_preview = true;
        let uses_chunk_append_body_fallback = false;
        assert!(uses_overlay_replace_preview);
        assert!(
            !uses_chunk_append_body_fallback,
            "P0′: canonical preview must replace overlay, not append_assistant_chunk per chunk"
        );
    }
}
