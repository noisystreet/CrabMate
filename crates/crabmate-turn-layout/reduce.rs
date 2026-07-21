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
        // 阶段 5c：Answer 段正文不再写入 canonical `final_answer`（字段已删除）；
        // 终答正文由 overlay 承载。段关闭后直接丢弃正文。
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
    }
}

/// 形态 B：按终答分隔符拆回 batch + final（纯函数，保留供未来复用）。
#[allow(dead_code)]
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

/// 形态 B 巨泡：短终答开头、后接 batch 旁注（纯函数，保留供未来复用）。
#[allow(dead_code)]
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

/// 无「总结：」等标记时：将末尾一句短终答从 batch 中剥离（纯函数，保留供未来复用）。
#[allow(dead_code)]
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

/// 判断文本是否像 markdown blockquote 的署名行（纯函数，保留供未来复用）。
#[allow(dead_code)]
fn looks_like_blockquote_attribution(text: &str) -> bool {
    let t = text.trim();
    t.starts_with("— ") || t.starts_with("-- ") || (t.starts_with('—') && !t.contains('。'))
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
}
