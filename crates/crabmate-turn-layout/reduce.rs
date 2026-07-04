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
