//! 流结束投影收口（Phase C：Loading 纯句柄）。

use leptos::prelude::GetUntracked;

use crate::stream_text_overlay::{
    stream_overlay_answer_for_message, stream_overlay_clear_answer_for_message,
};

use super::super::super::context::ChatStreamCallbackCtx;
use super::turn_row_queue::{FINAL_ANSWER_ROW_ID, TurnRowQueue};

/// `on_done` 前收口（Phase C：Loading 纯句柄）。
///
/// **顺序**：调用方须先 `sync_turn_projection`（`flush_final_answer_row` 读 overlay）。
/// 本函数：若终答行仍缺失则从 overlay（或残留 loading.text）补建；然后清空 overlay 与
/// loading 正文，**禁止**把终答长期停在 loading 壳上再升格。
pub(super) fn drain_stream_tail_into_canonical_for_done(stream_ctx: &ChatStreamCallbackCtx) {
    let mid = stream_ctx.scratch.clone_assistant_id();
    let sid = stream_ctx.bound_stream_session_id.clone();
    let overlay_answer = stream_overlay_answer_for_message(
        stream_ctx.chat.stream_text_overlay.get_untracked().as_ref(),
        sid.as_str(),
        mid.as_str(),
    );
    stream_ctx.update_bound_session(|s| {
        let has_final = s
            .messages
            .iter()
            .any(|m| m.id == FINAL_ANSWER_ROW_ID && !m.text.trim().is_empty());
        if !has_final {
            let from_overlay = overlay_answer
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string);
            let from_loading = s
                .messages
                .iter()
                .find(|m| m.id == mid.as_str())
                .map(|m| m.text.clone())
                .filter(|t| !t.trim().is_empty());
            if let Some(text) = from_overlay.or(from_loading) {
                TurnRowQueue::ensure_final_answer_row_from_text(
                    &mut s.messages,
                    &text,
                    Some(mid.as_str()),
                );
            }
        }
        stream_overlay_clear_answer_for_message(
            stream_ctx.chat.stream_text_overlay,
            sid.as_str(),
            mid.as_str(),
            Some(stream_ctx.chat.stream_overlay_revision),
        );
        if let Some(idx) = s.messages.iter().position(|m| m.id == mid.as_str()) {
            s.messages[idx].text.clear();
        }
    });
}

/// 流结束：先关 open 段 → `sync_turn_projection`（投影 FINAL_ANSWER_ROW）→ `drain` 收口。
///
/// **顺序不变量**：`sync_turn_projection` 必须在前，`drain` 在后。
/// `flush_final_answer_row` 从 overlay 读取终答正文；`drain` 在投影之后清空 overlay /
/// loading 句柄（Phase C，不再 `take` 进壳再升格）。若先 drain 再 sync，则
/// FINAL_ANSWER_ROW 读到空 overlay 而落盘不全。
pub(super) fn finalize_turn_projection_before_stream_done_inner(
    stream_ctx: &ChatStreamCallbackCtx,
) {
    if stream_ctx.scratch.tool_phase_open() {
        stream_ctx.scratch.on_turn_tool_phase_end();
    } else {
        stream_ctx.scratch.close_open_commentary_for_projection();
        stream_ctx.scratch.close_post_tool_final_answer_gate();
    }
    // sync_turn_projection 在前：flush_final_answer_row 需读 overlay 创建 FINAL_ANSWER_ROW。
    // drain 在后：清 overlay + loading 句柄；顺序不可交换，否则 FINAL_ANSWER_ROW 落盘不全。
    // projecting_stream_end：与形态 B 终答门解耦，仅在此窗口允许刷终答（避免中间过程
    // 因「已有工具行」误写 turn-final-answer 与 commentary/loading 双写）。
    stream_ctx.scratch.set_projecting_stream_end(true);
    stream_ctx.scratch.sync_turn_projection(stream_ctx);
    stream_ctx.scratch.set_projecting_stream_end(false);
    drain_stream_tail_into_canonical_for_done(stream_ctx);
}
