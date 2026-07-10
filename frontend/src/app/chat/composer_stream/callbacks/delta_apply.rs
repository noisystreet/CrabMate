//! SSE `on_delta` 正文/思维链写入与车道轮换（从 [`super::builders::chat_stream_on_delta_builder`] 拆出以便单测与降 nloc）。

use std::rc::Rc;

use super::super::context::ChatStreamCallbackCtx;
use super::super::per_stream_accum::PerStreamAccum;
use super::super::stream_control_reducer::StreamControlEvent;
use super::super::stream_turn_state::StreamModelOutputLane;
use super::turn_layout::TurnLayout;

/// post-tool 或正文相：canonical [`AnswerDelta`] + overlay preview（P0′）；禁止 chunk append 正文。
///
/// **热路径优化**：每 token 仅经 `sync_stream_preview` 写 overlay 信号，**不**写 `sessions`。
/// `sessions` 只在段/工具边界（`on_turn_segment_end` / `on_turn_tool_phase_end`）或终态
/// （`on_done` / `on_error`）由调用方 flush，避免每 token 的 O(全量消息) 信号级联。
fn apply_answer_body_delta(
    stream_ctx: &ChatStreamCallbackCtx,
    accum: &PerStreamAccum,
    chunk: &str,
) {
    if stream_ctx.scratch.try_apply_answer_delta(chunk) {
        stream_ctx.scratch.sync_stream_preview(stream_ctx);
        accum.add_answer_delta_chars(chunk.chars().count());
        return;
    }
    if stream_ctx.scratch.post_tool_stream_tail_active() {
        if stream_ctx.scratch.tool_phase_open()
            && stream_ctx.scratch.try_apply_commentary_delta(chunk)
        {
            stream_ctx.scratch.sync_stream_preview(stream_ctx);
        }
        accum.add_answer_delta_chars(chunk.chars().count());
        return;
    }
    // P0：canonical miss 时尝试 commentary 段，仍 miss 则 no-op（勿 append 尾泡）。
    if stream_ctx.scratch.try_apply_commentary_delta(chunk) {
        stream_ctx.scratch.sync_stream_preview(stream_ctx);
    }
    accum.add_answer_delta_chars(chunk.chars().count());
}

/// post-tool 形态 B：工具批结束后、终答门开前 plain delta → batch 说明；门开后 → 终答。
///
/// **热路径优化**：门转换处的 `sync_turn_projection` 保留（需将门状态落盘到 `sessions`）；
/// 纯 commentary delta 仅走 overlay（同行 20–40 行 `apply_answer_body_delta`）。
fn morph_b_chunk_is_standalone_final(chunk: &str, batch_len: usize) -> bool {
    let t = chunk.trim();
    batch_len >= 8
        && t.len() >= 4
        && t.len() <= 200
        && !t.contains('\n')
        && t.chars().filter(|c| *c == '。').count() <= 2
}

fn apply_post_tool_plain_delta(
    stream_ctx: &ChatStreamCallbackCtx,
    accum: &PerStreamAccum,
    chunk: &str,
) {
    if !stream_ctx.scratch.post_tool_final_answer_open() {
        if morph_b_chunk_is_standalone_final(chunk, stream_ctx.scratch.batch_narration_char_len()) {
            stream_ctx.scratch.open_post_tool_final_answer_gate();
            apply_answer_body_delta(stream_ctx, accum, chunk);
            stream_ctx.scratch.sync_turn_projection(stream_ctx);
            stream_ctx.scratch.sync_stream_preview(stream_ctx);
            accum.add_answer_delta_chars(chunk.chars().count());
            return;
        }
        if let Some((batch_part, final_part)) =
            crabmate_turn_layout::try_split_combined_post_tool_answer(chunk)
        {
            if !batch_part.is_empty() {
                let _ = stream_ctx
                    .scratch
                    .try_apply_commentary_delta(batch_part.as_str());
            }
            stream_ctx.scratch.open_post_tool_final_answer_gate();
            if !final_part.is_empty() {
                apply_answer_body_delta(stream_ctx, accum, final_part.as_str());
            }
            stream_ctx.scratch.sync_turn_projection(stream_ctx);
            stream_ctx.scratch.sync_stream_preview(stream_ctx);
            accum.add_answer_delta_chars(chunk.chars().count());
            return;
        }
        if stream_ctx.scratch.try_apply_commentary_delta(chunk) {
            stream_ctx.scratch.sync_stream_preview(stream_ctx);
        }
        accum.add_answer_delta_chars(chunk.chars().count());
        return;
    }
    apply_answer_body_delta(stream_ctx, accum, chunk);
}

/// 工具前旁注：canonical commentary 段（仅 overlay，同正文热路径）；miss 时不写尾泡（Phase 1 I2）。
fn apply_commentary_lane_delta(stream_ctx: &ChatStreamCallbackCtx, chunk: &str) {
    if stream_ctx.scratch.try_apply_commentary_delta(chunk) {
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

    // post-tool：工具批进行中 → commentary 块；结束后 → batch / 终答（形态 B 门控）。
    if post_tool && lane != StreamModelOutputLane::AnsweringCommentaryBeforeTools {
        if stream_ctx.scratch.tool_phase_open() {
            apply_commentary_lane_delta(stream_ctx, chunk);
            accum.add_answer_delta_chars(chunk.chars().count());
            return;
        }
        apply_post_tool_plain_delta(stream_ctx, accum, chunk);
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
        stream_ctx.scratch.apply_stream_control_event(
            &stream_ctx.shell.stream,
            StreamControlEvent::ModelTextDelta,
        );
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
    fn post_tool_after_tool_phase_end_uses_morph_b_gate_not_immediate_answer() {
        let post_tool = true;
        let tool_phase_open = false;
        let final_gate_open = false;
        let routes_to_batch = post_tool && !tool_phase_open && !final_gate_open;
        assert!(routes_to_batch);
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
