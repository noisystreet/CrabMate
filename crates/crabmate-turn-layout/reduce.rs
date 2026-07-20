use crate::event::TurnEvent;
use crate::model::{
    PENDING_STREAM_COMMENTARY_SEGMENT_ID, SegmentKind, ToolStep, Turn, TurnSegment,
};

#[derive(Debug, Default)]
pub struct TurnReducer;

fn attach_closed_commentary_to_step(turn: &mut Turn, before_tool_call_id: &str, text: String) {
    if text.trim().is_empty() {
        return;
    }
    if let Some(step) = turn.step_by_call_id_mut(before_tool_call_id) {
        match &mut step.before_commentary {
            Some(existing) => {
                existing.push_str(&text);
            }
            None => {
                step.before_commentary = Some(text);
            }
        }
        return;
    }
    turn.segments.push(TurnSegment {
        segment_id: format!("pending-before-{before_tool_call_id}"),
        kind: SegmentKind::Commentary,
        before_tool_call_id: Some(before_tool_call_id.to_string()),
        text,
        open: false,
    });
}

fn flush_segments_onto_steps(turn: &mut Turn) {
    let mut pending = Vec::new();
    turn.segments.retain(|s| {
        let take = !s.open
            && s.kind == SegmentKind::Commentary
            && s.before_tool_call_id.is_some()
            && !s.text.trim().is_empty();
        if take {
            pending.push(s.clone());
            false
        } else {
            true
        }
    });
    for seg in pending {
        if let Some(ref tid) = seg.before_tool_call_id {
            attach_closed_commentary_to_step(turn, tid, seg.text);
        }
    }
}

fn take_pending_stream_commentary(turn: &mut Turn) -> Option<String> {
    let idx = turn.segments.iter().position(|s| {
        s.segment_id == PENDING_STREAM_COMMENTARY_SEGMENT_ID && !s.text.trim().is_empty()
    })?;
    let seg = turn.segments.remove(idx);
    Some(seg.text)
}

fn close_open_commentary_except(turn: &mut Turn, except_id: Option<&str>) {
    let ids: Vec<String> = turn
        .segments
        .iter()
        .filter(|s| {
            s.open && s.kind == SegmentKind::Commentary && except_id != Some(s.segment_id.as_str())
        })
        .map(|s| s.segment_id.clone())
        .collect();
    for id in ids {
        reduce_segment_end(turn, id);
    }
}

fn close_all_open_commentary_segments(turn: &mut Turn) {
    close_open_commentary_except(turn, None);
}

/// 流结束 / 投影前：关闭仍 open 的 commentary 段并 flush 到 step（不切换 `tool_phase_open`）。
pub fn close_open_commentary_segments(turn: &mut Turn) {
    close_all_open_commentary_segments(turn);
    flush_segments_onto_steps(turn);
}

fn reduce_segment_delta(turn: &mut Turn, segment_id: String, delta: String) {
    if delta.is_empty() {
        return;
    }
    if let Some(seg) = turn.segment_by_id_mut(&segment_id) {
        seg.text.push_str(&delta);
        return;
    }
    if let Some(tid) = segment_id.strip_prefix("seg-before-") {
        attach_closed_commentary_to_step(turn, tid, delta);
    }
}

fn reduce_segment_end(turn: &mut Turn, segment_id: String) {
    let Some(idx) = turn
        .segments
        .iter()
        .position(|s| s.segment_id == segment_id)
    else {
        return;
    };
    let mut seg = turn.segments.remove(idx);
    seg.open = false;
    if seg.kind == SegmentKind::Answer {
        let t = seg.text.trim();
        if !t.is_empty() {
            turn.final_answer = Some(seg.text);
        }
        return;
    }
    if seg.segment_id == PENDING_STREAM_COMMENTARY_SEGMENT_ID {
        turn.segments.push(seg);
        return;
    }
    if let Some(ref tid) = seg.before_tool_call_id {
        attach_closed_commentary_to_step(turn, tid, seg.text);
    } else {
        turn.segments.push(seg);
    }
}

fn reduce_segment_start(
    turn: &mut Turn,
    segment_id: String,
    kind: SegmentKind,
    before_tool_call_id: Option<String>,
) {
    if let Some(existing) = turn.segment_by_id_mut(&segment_id) {
        if existing.open {
            close_open_commentary_except(turn, Some(segment_id.as_str()));
            return;
        }
        existing.open = true;
        close_open_commentary_except(turn, Some(segment_id.as_str()));
        return;
    }
    close_open_commentary_except(turn, None);
    turn.segments.push(TurnSegment {
        segment_id,
        kind,
        before_tool_call_id,
        text: String::new(),
        open: true,
    });
}

fn close_open_segment_if_present(turn: &mut Turn, segment_id: &str) {
    if turn
        .segments
        .iter()
        .any(|s| s.segment_id == segment_id && s.open)
    {
        reduce_segment_end(turn, segment_id.to_string());
    }
}

fn reduce_tool_call(turn: &mut Turn, tool_call_id: String, name: String, summary: String) {
    turn.tool_phase_open = true;
    close_open_segment_if_present(turn, PENDING_STREAM_COMMENTARY_SEGMENT_ID);
    flush_segments_onto_steps(turn);
    let pending_stream = take_pending_stream_commentary(turn);
    let mut before_commentary = pending_stream.filter(|t| !t.trim().is_empty());
    let mut remain = Vec::new();
    for seg in turn.segments.drain(..) {
        if seg.kind == SegmentKind::Commentary
            && seg.before_tool_call_id.as_deref() == Some(tool_call_id.as_str())
            && !seg.text.trim().is_empty()
        {
            before_commentary = Some(match before_commentary {
                Some(mut s) => {
                    s.push_str(&seg.text);
                    s
                }
                None => seg.text,
            });
        } else {
            remain.push(seg);
        }
    }
    turn.segments = remain;
    turn.steps.push(ToolStep {
        tool_call_id,
        name,
        summary,
        before_commentary,
    });
}

pub fn reduce_event(turn: &mut Turn, event: TurnEvent) {
    match event {
        TurnEvent::TimelineAssistant { text } => {
            if !text.trim().is_empty() {
                turn.pre_tool_timeline.push(text);
            }
        }
        TurnEvent::SegmentStart {
            segment_id,
            kind,
            before_tool_call_id,
        } => reduce_segment_start(turn, segment_id, kind, before_tool_call_id),
        TurnEvent::SegmentDelta { segment_id, delta } => {
            reduce_segment_delta(turn, segment_id, delta);
        }
        TurnEvent::SegmentEnd { segment_id } => reduce_segment_end(turn, segment_id),
        TurnEvent::ToolCall {
            tool_call_id,
            name,
            summary,
        } => reduce_tool_call(turn, tool_call_id, name, summary),
        TurnEvent::ToolPhaseEnd => {
            close_all_open_commentary_segments(turn);
            turn.tool_phase_open = false;
            flush_segments_onto_steps(turn);
        }
        TurnEvent::AnswerDelta { delta } => {
            if delta.is_empty() || turn.tool_phase_open {
                return;
            }
            match &mut turn.final_answer {
                Some(a) => a.push_str(&delta),
                None => turn.final_answer = Some(delta),
            }
        }
    }
}

/// 形态 B：`turn_tool_phase_end` 后 plain delta 误入 `final_answer` 时，按终答分隔符拆回 batch + final。
#[must_use]
pub fn try_split_combined_post_tool_answer(combined: &str) -> Option<(String, String)> {
    const MARKERS: &[&str] = &[
        "\n\n**",
        "\n---\n",
        "。总结：",
        "。总结:",
        "。Summary:",
        "。summary:",
    ];
    let trimmed = combined.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut best: Option<(usize, &str)> = None;
    for marker in MARKERS {
        if let Some(pos) = trimmed.rfind(marker)
            && pos > 0
            && best.is_none_or(|(p, _)| pos > p)
        {
            best = Some((pos, marker));
        }
    }
    let (pos, marker) = best?;
    let head = trimmed[..pos].trim();
    let tail = trimmed[pos..].trim();
    let tail = tail.strip_prefix(marker).unwrap_or(tail).trim();
    if head.len() < 8 || tail.len() < 4 {
        return None;
    }
    Some((head.to_string(), tail.to_string()))
}

/// 形态 B 巨泡：`final_answer` 以短终答开头、后接 batch 旁注（流式误路由顺序）。
#[must_use]
fn try_split_leading_final_sentence(combined: &str) -> Option<(String, String)> {
    let trimmed = combined.trim();
    let (final_body, rest) = trimmed.split_once('。')?;
    let final_sent = format!("{}。", final_body.trim());
    let rest = rest.trim();
    let final_len = final_sent.chars().count();
    let looks_terminal = final_sent.contains("完成")
        || final_sent.contains("总结")
        || final_sent.contains("完毕")
        || final_len <= 16;
    if !(4..=32).contains(&final_len) || !looks_terminal || rest.len() < 12 || !rest.contains('。')
    {
        return None;
    }
    Some((rest.to_string(), final_sent))
}

/// 无「总结：」等标记时：将末尾一句短终答从 batch 中剥离（形态 B 桩 / 短 plain final）。
/// 仅在有 batch 旁注（`batch_was_empty=false`）时使用；`final_only` 场景下不调用，
/// 因为无 batch 时 `final_answer` 是模型完整正文，短句启发式太容易误判。
#[must_use]
fn try_peel_trailing_final_sentence(combined: &str) -> Option<(String, String)> {
    let trimmed = combined.trim().trim_end_matches('。');
    let (head, tail_body) = trimmed.rsplit_once('。')?;
    let head = format!("{}。", head.trim());
    let tail = format!("{}。", tail_body.trim());
    if head.len() < 12 || tail.len() < 4 || tail.len() > 240 {
        return None;
    }
    if !head.contains('。') || head.len() <= tail.len() {
        return None;
    }
    // tail 以 em-dash 署名行开头时不拆分
    // （如 "...复杂。 — 列奥纳多·达·芬奇。" 不应把 "— 列奥纳多·达·芬奇。" 拆为终答）
    if looks_like_blockquote_attribution(&tail) {
        return None;
    }
    Some((head, tail))
}

/// 判断文本是否像 markdown blockquote 的署名行（如 "— 列奥纳多·达·芬奇。"、"— Albert Einstein。"）。
fn looks_like_blockquote_attribution(text: &str) -> bool {
    let t = text.trim();
    t.starts_with("— ") || t.starts_with("-- ") || (t.starts_with('—') && !t.contains('。'))
}

fn apply_repartition_split(turn: &mut Turn, batch_text: String, final_text: String) {
    clear_batch_commentary_from_turn(turn);
    let attached = attach_batch_text_to_turn(turn, &batch_text);
    if !attached && !batch_text.trim().is_empty() {
        // 零工具轮次（无 tool step 可附着）：batch 段合并回 final_answer，
        // 避免 `flush_final_answer_row` 因 `batch_projection_pending` 跳过。
        turn.final_answer = Some(merge_batch_and_final(&batch_text, &final_text));
    } else {
        turn.final_answer = Some(final_text);
    }
}

/// 将批说明文本附着到首个 tool step（若存在）。
/// 返回 `true` 表示成功附着到 tool step；`false` 表示无 tool step（零工具场景），
/// 调用方应将 batch 文本合并回 `final_answer`。
fn attach_batch_text_to_turn(turn: &mut Turn, text: &str) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    if let Some(first_id) = turn.steps.first().map(|s| s.tool_call_id.clone()) {
        attach_closed_commentary_to_step(turn, first_id.as_str(), text.to_string());
        return true;
    }
    // 零工具场景：不推入 segment（否则 `batch_narration_text` 会找到，导致
    // `batch_projection_pending_in_messages` 阻塞 `flush_final_answer_row`）。
    false
}

fn try_repartition_combined_text(combined: &str) -> Option<(String, String)> {
    try_split_combined_post_tool_answer(combined)
        .or_else(|| try_split_leading_final_sentence(combined))
        .or_else(|| try_peel_trailing_final_sentence(combined))
}

/// `final_only` 场景（无 batch 旁注）下的受限拆分：仅使用明确标记（`\n\n**` 等）拆分，
/// 不使用基于短句位置启发式的 `try_split_leading_final_sentence` / `try_peel_trailing_final_sentence`。
fn try_repartition_final_only_text(combined: &str) -> Option<(String, String)> {
    try_split_combined_post_tool_answer(combined)
}

fn clear_batch_commentary_from_turn(turn: &mut Turn) {
    for step in &mut turn.steps {
        step.before_commentary = None;
    }
    turn.segments.retain(|s| {
        !(s.kind == SegmentKind::Commentary
            && (s.before_tool_call_id.is_some()
                || s.segment_id == PENDING_STREAM_COMMENTARY_SEGMENT_ID))
    });
}

/// 合并 batch 与 final_part 为 combined 文本（处理重叠/包含关系）。
fn merge_batch_and_final(batch: &str, final_part: &str) -> String {
    if final_part.trim().is_empty() {
        return batch.to_string();
    }
    let trimmed_batch = batch.trim();
    if trimmed_batch.is_empty()
        || final_part.starts_with(trimmed_batch)
        || final_part.contains(trimmed_batch)
    {
        final_part.to_string()
    } else {
        let mut combined = batch.to_string();
        combined.push_str(final_part);
        combined
    }
}

/// Web 块布局：`on_done` / 投影前将 batch + `final_answer` 巨泡拆成独立 batch 与终答。
pub fn repartition_web_block_layout_stream(turn: &mut Turn) {
    if turn.tool_phase_open {
        return;
    }
    close_all_open_commentary_segments(turn);
    let batch = crate::batch_narration_text(turn).unwrap_or_default();
    let batch_was_empty = batch.trim().is_empty();
    let final_part = turn.final_answer.take().unwrap_or_default();
    // Segment 布局：batch 与 final 已分列时不做 morph B 巨泡拆分（避免误拆 `try_split_leading_final_sentence`）。
    if !batch_was_empty && !final_part.trim().is_empty() {
        let b = batch.trim();
        let f = final_part.trim();
        if b == f {
            // 完全相同：保留终答，清除批说明（避免导出双段）。
            clear_batch_commentary_from_turn(turn);
            turn.final_answer = Some(final_part);
            return;
        }
        if !f.starts_with(b) && !b.contains(f) && !f.contains(b) {
            turn.final_answer = Some(final_part);
            return;
        }
    }
    let final_only = !final_part.trim().is_empty() && batch_was_empty;
    let combined = merge_batch_and_final(&batch, &final_part);
    if combined.trim().is_empty() {
        return;
    }
    if let Some((batch_text, final_text)) = if final_only {
        try_repartition_final_only_text(&combined)
    } else {
        try_repartition_combined_text(&combined)
    } {
        apply_repartition_split(turn, batch_text, final_text);
        return;
    }
    // 无法拆分：保留 batch 结构；勿把整段写入 final_answer（会与 batch 行重复成巨泡）。
    if final_only {
        turn.final_answer = Some(combined);
    }
}

impl TurnReducer {
    pub fn apply(&self, turn: &mut Turn, event: TurnEvent) {
        reduce_event(turn, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SegmentKind;

    #[test]
    fn late_commentary_delta_attaches_to_prior_tool_step() {
        let mut turn = Turn::default();
        let r = TurnReducer;
        r.apply(
            &mut turn,
            TurnEvent::ToolCall {
                tool_call_id: "tc_read".into(),
                name: "read_dir".into(),
                summary: "read dir".into(),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentStart {
                segment_id: "seg-before-tc_create".into(),
                kind: SegmentKind::Commentary,
                before_tool_call_id: Some("tc_create".into()),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::ToolCall {
                tool_call_id: "tc_create".into(),
                name: "create_file".into(),
                summary: "create file".into(),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentDelta {
                segment_id: "seg-before-tc_create".into(),
                delta: "工作区是空的。".into(),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentEnd {
                segment_id: "seg-before-tc_create".into(),
            },
        );
        let step = turn.step_by_call_id_mut("tc_create").unwrap();
        assert_eq!(step.before_commentary.as_deref(), Some("工作区是空的。"));
    }

    #[test]
    fn tool_call_closes_pending_stream_not_tool_segment() {
        let mut turn = Turn::default();
        let r = TurnReducer;
        r.apply(
            &mut turn,
            TurnEvent::SegmentStart {
                segment_id: "seg-before-tc_a".into(),
                kind: SegmentKind::Commentary,
                before_tool_call_id: Some("tc_a".into()),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentDelta {
                segment_id: "seg-before-tc_a".into(),
                delta: "步骤 A。".into(),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::ToolCall {
                tool_call_id: "tc_a".into(),
                name: "tool_a".into(),
                summary: "tool a".into(),
            },
        );
        assert_eq!(
            turn.step_by_call_id("tc_a")
                .and_then(|s| s.before_commentary.as_deref()),
            Some("步骤 A。")
        );
    }

    #[test]
    fn pending_stream_commentary_attaches_to_first_tool_call() {
        let mut turn = Turn::default();
        let r = TurnReducer;
        r.apply(
            &mut turn,
            TurnEvent::SegmentStart {
                segment_id: PENDING_STREAM_COMMENTARY_SEGMENT_ID.into(),
                kind: SegmentKind::Commentary,
                before_tool_call_id: None,
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentDelta {
                segment_id: PENDING_STREAM_COMMENTARY_SEGMENT_ID.into(),
                delta: "先解压。".into(),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::ToolCall {
                tool_call_id: "tc_unpack".into(),
                name: "unpack".into(),
                summary: "unpack".into(),
            },
        );
        let step = turn.step_by_call_id("tc_unpack").unwrap();
        assert_eq!(step.before_commentary.as_deref(), Some("先解压。"));
    }

    #[test]
    fn tool_phase_end_closes_open_commentary_into_batch() {
        let mut turn = Turn::default();
        let r = TurnReducer;
        r.apply(
            &mut turn,
            TurnEvent::SegmentStart {
                segment_id: PENDING_STREAM_COMMENTARY_SEGMENT_ID.into(),
                kind: SegmentKind::Commentary,
                before_tool_call_id: None,
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentDelta {
                segment_id: PENDING_STREAM_COMMENTARY_SEGMENT_ID.into(),
                delta: "先看安装说明。".into(),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::ToolCall {
                tool_call_id: "tc_read".into(),
                name: "read_file".into(),
                summary: "read INSTALL".into(),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentStart {
                segment_id: PENDING_STREAM_COMMENTARY_SEGMENT_ID.into(),
                kind: SegmentKind::Commentary,
                before_tool_call_id: None,
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentDelta {
                segment_id: PENDING_STREAM_COMMENTARY_SEGMENT_ID.into(),
                delta: "继续读 Makefile。".into(),
            },
        );
        assert!(
            turn.segments
                .iter()
                .any(|s| s.segment_id == PENDING_STREAM_COMMENTARY_SEGMENT_ID && s.open),
            "mid-tool commentary stays open until tool_phase_end"
        );
        r.apply(&mut turn, TurnEvent::ToolPhaseEnd);
        assert!(
            crate::streaming_commentary_block_text(&turn).is_none(),
            "open preview must be empty after tool_phase_end"
        );
        let batch = crate::batch_narration_text(&turn).expect("batch");
        assert!(batch.contains("先看安装说明。") && batch.contains("继续读 Makefile。"));
    }

    #[test]
    fn leading_final_split_fixes_prepended_mega_bubble() {
        let combined = "HPCG 编译完成。好的，先解压 HPCG 看看结构。HPCG 源码已解压。开始编译。";
        let (batch, fin) = try_split_leading_final_sentence(combined).expect("split");
        assert!(batch.contains("先解压"));
        assert_eq!(fin, "HPCG 编译完成。");
    }

    #[test]
    fn repartition_skips_disjoint_segment_batch_and_final() {
        let mut turn = Turn::default();
        let r = TurnReducer;
        r.apply(
            &mut turn,
            TurnEvent::ToolCall {
                tool_call_id: "tc_archive".into(),
                name: "archive_list".into(),
                summary: "list".into(),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentStart {
                segment_id: "seg-before-tc_unpack".into(),
                kind: SegmentKind::Commentary,
                before_tool_call_id: Some("tc_unpack".into()),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentDelta {
                segment_id: "seg-before-tc_unpack".into(),
                delta: "好的，先解压。".into(),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentEnd {
                segment_id: "seg-before-tc_unpack".into(),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::ToolCall {
                tool_call_id: "tc_unpack".into(),
                name: "unpack".into(),
                summary: "unpack".into(),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentStart {
                segment_id: "seg-before-tc_read".into(),
                kind: SegmentKind::Commentary,
                before_tool_call_id: Some("tc_read".into()),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentDelta {
                segment_id: "seg-before-tc_read".into(),
                delta: "读取 INSTALL。".into(),
            },
        );
        r.apply(&mut turn, TurnEvent::ToolPhaseEnd);
        turn.final_answer = Some("HPCG 编译流程结束。".into());
        let batch_before = crate::batch_narration_text(&turn).expect("batch");
        repartition_web_block_layout_stream(&mut turn);
        assert_eq!(
            crate::batch_narration_text(&turn).as_deref(),
            Some(batch_before.as_str())
        );
        assert_eq!(turn.final_answer.as_deref(), Some("HPCG 编译流程结束。"));
    }

    #[test]
    fn repartition_splits_real_morph_b_mega_stream_at_summary_marker() {
        let mut turn = Turn::default();
        let r = TurnReducer;
        r.apply(
            &mut turn,
            TurnEvent::ToolCall {
                tool_call_id: "tc_make".into(),
                name: "run_command".into(),
                summary: "make".into(),
            },
        );
        r.apply(&mut turn, TurnEvent::ToolPhaseEnd);
        let combined = "好的，我来编译 HPCG。先看看配置。已清理。现在重新编译：编译成功！HPCG 编译完成。总结：\
                        \n\n**编译配置**\n- arch=GCC_OMP";
        turn.final_answer = Some(combined.to_string());
        repartition_web_block_layout_stream(&mut turn);
        let batch = crate::batch_narration_text(&turn).expect("batch");
        assert!(batch.contains("好的，我来编译 HPCG"));
        assert!(batch.contains("编译成功"));
        assert!(
            !batch.contains("**编译配置**"),
            "summary must not stay in batch"
        );
        let final_a = turn.final_answer.as_deref().expect("final");
        assert!(final_a.contains("**编译配置**") || final_a.contains("总结"));
    }

    #[test]
    fn repartition_peels_morph_b_stub_final_without_summary_marker() {
        let mut turn = Turn::default();
        let r = TurnReducer;
        r.apply(
            &mut turn,
            TurnEvent::ToolCall {
                tool_call_id: "tc_make".into(),
                name: "run_command".into(),
                summary: "make".into(),
            },
        );
        r.apply(&mut turn, TurnEvent::ToolPhaseEnd);
        let combined = "好的，先解压 HPCG 看看结构。HPCG 源码已解压。读取 INSTALL 与 Makefile。开始编译。HPCG 编译完成。";
        turn.final_answer = None;
        attach_batch_text_to_turn(&mut turn, combined);
        repartition_web_block_layout_stream(&mut turn);
        let batch = crate::batch_narration_text(&turn).expect("batch");
        assert!(batch.contains("开始编译"));
        assert!(!batch.contains("HPCG 编译完成"));
        assert_eq!(turn.final_answer.as_deref(), Some("HPCG 编译完成。"));
    }

    #[test]
    fn repartition_does_not_duplicate_mega_into_final_when_split_fails() {
        let mut turn = Turn::default();
        let r = TurnReducer;
        r.apply(
            &mut turn,
            TurnEvent::ToolCall {
                tool_call_id: "tc_a".into(),
                name: "list_tree".into(),
                summary: "list".into(),
            },
        );
        r.apply(&mut turn, TurnEvent::ToolPhaseEnd);
        attach_batch_text_to_turn(&mut turn, "只有批说明没有终答标记也没有独立尾句");
        repartition_web_block_layout_stream(&mut turn);
        assert!(turn.final_answer.is_none());
        assert!(crate::batch_narration_text(&turn).is_some_and(|t| t.contains("只有批说明")));
    }

    #[test]
    fn peel_trailing_does_not_split_blockquote_attribution() {
        let combined = "已在 `agent_space/quote.md` 中写入：\n\n> **\"Simplicity is the ultimate sophistication.\"** — Leonardo da Vinci\n>\n> 简洁是终极的复杂。 — 列奥纳多·达·芬奇。";
        assert_eq!(try_peel_trailing_final_sentence(combined), None);
    }

    #[test]
    fn peel_trailing_does_not_split_em_dash_attribution() {
        let combined = "好的，先看看结构。读取了 INSTALL 文件。 — Albert Einstein。";
        assert_eq!(try_peel_trailing_final_sentence(combined), None);
    }

    #[test]
    fn peel_trailing_still_splits_normal_terminal_sentence() {
        let combined = "好的，先解压 HPCG 看看结构。HPCG 源码已解压。读取 INSTALL 与 Makefile。开始编译。HPCG 编译完成。";
        let (batch, fin) = try_peel_trailing_final_sentence(combined).expect("should split");
        assert!(batch.contains("开始编译"));
        assert_eq!(fin, "HPCG 编译完成。");
    }

    #[test]
    fn segment_start_closes_other_open_commentary_segments() {
        let mut turn = Turn::default();
        let r = TurnReducer;
        r.apply(
            &mut turn,
            TurnEvent::SegmentStart {
                segment_id: "seg-before-a".into(),
                kind: SegmentKind::Commentary,
                before_tool_call_id: Some("a".into()),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentDelta {
                segment_id: "seg-before-a".into(),
                delta: "for a".into(),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentStart {
                segment_id: "seg-before-b".into(),
                kind: SegmentKind::Commentary,
                before_tool_call_id: Some("b".into()),
            },
        );
        assert!(
            turn.segments
                .iter()
                .all(|s| s.segment_id != "seg-before-a" || !s.open)
        );
        assert!(
            turn.segments
                .iter()
                .any(|s| s.segment_id == "seg-before-b" && s.open)
        );
    }

    // -----------------------------------------------------------------------
    // `repartition_web_block_layout_stream`：零工具场景不损坏 final_answer
    // -----------------------------------------------------------------------

    #[test]
    fn repartition_zero_tool_keeps_final_answer_unchanged() {
        let mut turn = Turn::default();
        let r = TurnReducer;
        r.apply(
            &mut turn,
            TurnEvent::AnswerDelta {
                delta: "我具备以下技能，按类别整理如下：\n\n---\n\n### 📁 文件与目录操作\n- **读写文件**：`read_file`、`create_file`\n\n---\n\n需要我帮你做什么？可以直接说任务。".into(),
            },
        );
        repartition_web_block_layout_stream(&mut turn);
        let after = turn.final_answer.as_deref().unwrap_or("");
        assert!(!after.is_empty(), "零工具轮次 final_answer 不应被清空");
        assert!(after.contains("文件与目录操作"), "终答应保留核心内容");
        assert!(after.contains("需要我帮你做什么？"), "终答应保留结尾");
    }

    #[test]
    fn repartition_zero_tool_answer_not_doubled_from_streaming() {
        // 模拟流式期间累积的 final_answer（无工具轮次，无 batch commentary）
        let mut turn = Turn::default();
        let r = TurnReducer;
        let answer = "我具备以下技能，按类别整理如下：\n\n---\n\n### 📁 文件与目录操作\n- **读写文件**：`read_file`、`create_file`\n\n---\n\n需要我帮你做什么？可以直接说任务。";
        // 按字符位置模拟流式逐 delta 累积
        let chars: Vec<char> = answer.chars().collect();
        let third = chars.len() / 3;
        for chunk in [
            &chars[..third],
            &chars[third..2 * third],
            &chars[2 * third..],
        ] {
            let delta: String = chunk.iter().collect();
            r.apply(&mut turn, TurnEvent::AnswerDelta { delta });
        }
        repartition_web_block_layout_stream(&mut turn);
        let after = turn.final_answer.as_deref().unwrap_or("");
        // 验证总长度不超过原文 1.1 倍（无加倍）
        assert!(
            after.len() <= (answer.len() as f64 * 1.1) as usize + 5,
            "final_answer 不应加倍: len={}, expected≤{}",
            after.len(),
            (answer.len() as f64 * 1.1) as usize
        );
        assert!(after.contains("文件与目录操作"), "终答应保留核心内容");
    }
}
