use crate::model::{PENDING_STREAM_COMMENTARY_SEGMENT_ID, SegmentKind, Turn};

/// Web 块布局：`assistant_batch_narration` 行 kind（与 `project_turn_web` / 金样一致）。
pub const ASSISTANT_BATCH_NARRATION: &str = "assistant_batch_narration";
/// Web 块布局：终答行 kind。
pub const ASSISTANT_ANSWER: &str = "assistant_answer";

/// 合并 `step.before_commentary` 与同锚点未 flush 段，供 Web sync 即时投影。
#[must_use]
pub fn commentary_for_tool(turn: &Turn, tool_call_id: &str) -> Option<String> {
    let mut text = turn
        .step_by_call_id(tool_call_id)
        .and_then(|s| s.before_commentary.clone())
        .unwrap_or_default();
    for seg in &turn.segments {
        if seg.kind == SegmentKind::Commentary
            && seg.before_tool_call_id.as_deref() == Some(tool_call_id)
            && !seg.text.is_empty()
        {
            text.push_str(&seg.text);
        }
    }
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProjectedRow {
    /// `assistant_timeline` | `assistant_commentary` | `assistant_batch_narration` | `assistant_answer` | `tool`
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

fn tool_row(step: &crate::model::ToolStep) -> ProjectedRow {
    ProjectedRow {
        kind: "tool".into(),
        text: step.summary.clone(),
        tool_name: Some(step.name.clone()),
        tool_call_id: Some(step.tool_call_id.clone()),
    }
}

fn row(kind: &str, text: impl Into<String>) -> ProjectedRow {
    ProjectedRow {
        kind: kind.to_string(),
        text: text.into(),
        tool_name: None,
        tool_call_id: None,
    }
}

/// 块布局：合并 **已关闭** pending / 锚点段 + 各 step `before_commentary`（open 段仅 overlay）。
#[must_use]
pub fn batch_narration_text(turn: &Turn) -> Option<String> {
    let mut out = String::new();
    for seg in &turn.segments {
        if seg.kind != SegmentKind::Commentary || seg.open || seg.text.trim().is_empty() {
            continue;
        }
        if seg.segment_id == PENDING_STREAM_COMMENTARY_SEGMENT_ID
            || seg.before_tool_call_id.is_some()
        {
            out.push_str(&seg.text);
        }
    }
    for step in &turn.steps {
        if let Some(ref c) = step.before_commentary
            && !c.trim().is_empty()
        {
            out.push_str(c);
        }
    }
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 工具批进行中：当前 **open** commentary 段（未落盘增量）。
#[must_use]
pub fn streaming_commentary_block_text(turn: &Turn) -> Option<String> {
    turn.segments
        .iter()
        .rev()
        .find(|s| s.open && s.kind == SegmentKind::Commentary && !s.text.is_empty())
        .map(|s| s.text.clone())
}

fn first_step_with_commentary_index(turn: &Turn) -> Option<usize> {
    turn.steps.iter().position(|s| {
        s.before_commentary
            .as_ref()
            .is_some_and(|t| !t.trim().is_empty())
    })
}

/// 将 canonical [`Turn`] 投影为聊天气泡顺序（纯函数；金样见 `fixtures/turn_project_golden.jsonl`）。
#[must_use]
pub fn project_turn(turn: &Turn) -> Vec<ProjectedRow> {
    let mut out = Vec::new();
    for t in &turn.pre_tool_timeline {
        out.push(row("assistant_timeline", t.clone()));
    }
    for step in &turn.steps {
        if let Some(ref c) = step.before_commentary
            && !c.trim().is_empty()
        {
            out.push(ProjectedRow {
                kind: "assistant_commentary".into(),
                text: c.clone(),
                tool_name: None,
                tool_call_id: Some(step.tool_call_id.clone()),
            });
        }
        out.push(ProjectedRow {
            kind: "tool".into(),
            text: step.summary.clone(),
            tool_name: Some(step.name.clone()),
            tool_call_id: Some(step.tool_call_id.clone()),
        });
    }
    if let Some(ref a) = turn.final_answer
        && !a.trim().is_empty()
    {
        out.push(row(ASSISTANT_ANSWER, a.clone()));
    }
    out
}

/// Web 块布局投影：无旁注工具 → 单条 `assistant_batch_narration` → 含旁注工具批 →（工具批结束后）`assistant_answer`。
#[must_use]
pub fn project_turn_web(turn: &Turn) -> Vec<ProjectedRow> {
    let mut out = Vec::new();
    let anchor_idx = first_step_with_commentary_index(turn);
    let batch = batch_narration_text(turn);

    for (i, step) in turn.steps.iter().enumerate() {
        if anchor_idx.is_some_and(|a| i < a) {
            out.push(tool_row(step));
        }
    }
    if let Some(text) = batch.clone() {
        out.push(ProjectedRow {
            kind: ASSISTANT_BATCH_NARRATION.into(),
            text,
            tool_name: None,
            tool_call_id: anchor_idx
                .and_then(|i| turn.steps.get(i).map(|s| s.tool_call_id.clone())),
        });
    }
    if let Some(a) = anchor_idx {
        for step in &turn.steps[a..] {
            out.push(tool_row(step));
        }
    } else {
        for step in &turn.steps {
            out.push(tool_row(step));
        }
    }
    if !turn.tool_phase_open
        && let Some(ref a) = turn.final_answer
        && !a.trim().is_empty()
    {
        out.push(row(ASSISTANT_ANSWER, a.clone()));
    }
    out
}

/// `project_turn_web` 中的批说明行（若有）。
#[must_use]
pub fn batch_narration_row(turn: &Turn) -> Option<ProjectedRow> {
    project_turn_web(turn)
        .into_iter()
        .find(|r| r.kind == ASSISTANT_BATCH_NARRATION)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::TurnEvent;
    use crate::model::SegmentKind;
    use crate::reduce::TurnReducer;

    #[test]
    fn commentary_for_tool_merges_step_and_pending_segment() {
        let mut turn = Turn::default();
        let r = TurnReducer;
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
                delta: "读取说明。".into(),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::ToolCall {
                tool_call_id: "tc_read".into(),
                name: "read_file".into(),
                summary: "read file".into(),
            },
        );
        assert_eq!(
            super::commentary_for_tool(&turn, "tc_read").as_deref(),
            Some("读取说明。")
        );
    }

    #[test]
    fn project_cpp_scenario_commentary_before_create() {
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
        r.apply(
            &mut turn,
            TurnEvent::ToolCall {
                tool_call_id: "tc_create".into(),
                name: "create_file".into(),
                summary: "create file".into(),
            },
        );
        r.apply(&mut turn, TurnEvent::ToolPhaseEnd);
        r.apply(
            &mut turn,
            TurnEvent::AnswerDelta {
                delta: "完成。".into(),
            },
        );
        let rows = project_turn(&turn);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].kind, "tool");
        assert_eq!(rows[1].kind, "assistant_commentary");
        assert_eq!(rows[1].text, "工作区是空的。");
        assert_eq!(rows[1].tool_call_id.as_deref(), Some("tc_create"));
        assert_eq!(rows[2].kind, "tool");
        assert_eq!(rows[3].kind, "assistant_answer");
    }

    #[test]
    fn batch_narration_includes_closed_segment_before_tool_call() {
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
                delta: "段已关闭。".into(),
            },
        );
        r.apply(
            &mut turn,
            TurnEvent::SegmentEnd {
                segment_id: "seg-before-tc_a".into(),
            },
        );
        assert_eq!(batch_narration_text(&turn).as_deref(), Some("段已关闭。"));
        assert!(streaming_commentary_block_text(&turn).is_none());
    }

    #[test]
    fn project_turn_web_hpcg_block_layout() {
        let mut turn = Turn::default();
        let r = TurnReducer;
        r.apply(
            &mut turn,
            TurnEvent::ToolCall {
                tool_call_id: "tc_archive".into(),
                name: "archive_list".into(),
                summary: "list archive".into(),
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
            TurnEvent::ToolCall {
                tool_call_id: "tc_unpack".into(),
                name: "unpack".into(),
                summary: "unpack archive".into(),
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
        r.apply(
            &mut turn,
            TurnEvent::ToolCall {
                tool_call_id: "tc_read".into(),
                name: "read_file".into(),
                summary: "read INSTALL".into(),
            },
        );
        let rows = project_turn_web(&turn);
        assert_eq!(rows[0].kind, "tool");
        assert_eq!(rows[0].tool_call_id.as_deref(), Some("tc_archive"));
        assert_eq!(rows[1].kind, "assistant_batch_narration");
        assert_eq!(rows[1].text, "好的，先解压。读取 INSTALL。");
        assert_eq!(rows[1].tool_call_id.as_deref(), Some("tc_unpack"));
        assert_eq!(rows[2].kind, "tool");
        assert_eq!(rows[2].tool_call_id.as_deref(), Some("tc_unpack"));
        assert_eq!(rows[3].kind, "tool");
        assert_eq!(rows[3].tool_call_id.as_deref(), Some("tc_read"));
    }
}
