use crate::cm_turn_layout::model::{PENDING_STREAM_COMMENTARY_SEGMENT_ID, SegmentKind, Turn};

/// Web 块布局：`assistant_batch_narration` 行 kind（与 `project_turn_web` / 金样一致）。
pub const ASSISTANT_BATCH_NARRATION: &str = "assistant_batch_narration";
/// Web v2：已关闭、锚定到工具调用的不可变旁注行。
pub const ASSISTANT_COMMENTARY: &str = "assistant_commentary";
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

fn tool_row(step: &crate::cm_turn_layout::model::ToolStep) -> ProjectedRow {
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

fn append_closed_batch_commentary(out: &mut String, turn: &Turn) {
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
}

fn append_step_before_commentary(out: &mut String, turn: &Turn) {
    for step in &turn.steps {
        if let Some(ref c) = step.before_commentary
            && !c.trim().is_empty()
        {
            out.push_str(c);
        }
    }
}

/// 块布局：合并 **已关闭** pending / 锚点段 + 各 step `before_commentary`（open 段仅 overlay）。
#[must_use]
pub fn batch_narration_text(turn: &Turn) -> Option<String> {
    let mut out = String::new();
    append_closed_batch_commentary(&mut out, turn);
    append_step_before_commentary(&mut out, turn);
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

/// 工具批进行中：open commentary 若带 `before_tool_call_id`，返回 `(tool_call_id, text)`。
///
/// 供 Web 在锚点工具行**已存在**时把流式旁白 upsert 到该工具之前，避免 loading 尾泡
/// 落在工具之后造成「工具先于描述」。
#[must_use]
pub fn streaming_commentary_before_tool(turn: &Turn) -> Option<(String, String)> {
    let seg = turn
        .segments
        .iter()
        .rev()
        .find(|s| s.open && s.kind == SegmentKind::Commentary && !s.text.trim().is_empty())?;
    let tool_call_id = seg
        .before_tool_call_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    Some((tool_call_id, seg.text.clone()))
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
    out
}

/// Web 块布局投影：无旁注工具 → 单条 `assistant_batch_narration` → 含旁注工具批。
/// 终答由 overlay 承载，`flush_final_answer_row` 从 overlay 参数读取。
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
    out
}

/// Web v2 投影：每个工具调用前的已关闭旁注独立成行。
///
/// `tool_call_id` 同时作为旁注行的稳定 projection key。已关闭旁注首次插入后正文可经
/// upsert 更新（晚到流式）；open segment 在锚点工具已存在时由 Web 侧 upsert 到工具前，
/// 不再仅挂 loading 尾泡。
#[must_use]
pub fn project_turn_web_v2(turn: &Turn) -> Vec<ProjectedRow> {
    let mut out = Vec::new();
    for timeline in &turn.pre_tool_timeline {
        out.push(row("assistant_timeline", timeline.clone()));
    }
    for step in &turn.steps {
        if let Some(commentary) = step
            .before_commentary
            .as_ref()
            .filter(|text| !text.trim().is_empty())
        {
            out.push(ProjectedRow {
                kind: ASSISTANT_COMMENTARY.into(),
                text: commentary.clone(),
                tool_name: None,
                tool_call_id: Some(step.tool_call_id.clone()),
            });
        }
        out.push(tool_row(step));
    }
    out
}

/// 流式 active 行：至多一条；有文才出现。不进入 [`TurnProjection::finalized_rows`]。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ActiveProjectedRow {
    /// `assistant_commentary` | `assistant_answer`
    pub kind: String,
    pub text: String,
    /// 锚定 open 旁白时的 `tool_call_id`；无锚点或终答为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_tool_call_id: Option<String>,
}

/// Canonical → UI 的显式投影（Phase D）：定稿行 + 可选 active。
///
/// - `finalized_rows`：与 [`project_turn_web_v2`] 相同（已关闭旁注 + 工具 + 时间线）
/// - `active_row`：当前 open commentary / open answer；Web reconciler 写入 commentary 行或 overlay
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TurnProjection {
    pub finalized_rows: Vec<ProjectedRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_row: Option<ActiveProjectedRow>,
}

fn streaming_open_answer_text(turn: &Turn) -> Option<String> {
    turn.segments
        .iter()
        .rev()
        .find(|s| s.open && s.kind == SegmentKind::Answer && !s.text.trim().is_empty())
        .map(|s| s.text.clone())
}

/// 定稿行 = [`project_turn_web_v2`]；active = open commentary 或 open answer（至多一条）。
#[must_use]
pub fn project_turn_projection(turn: &Turn) -> TurnProjection {
    let finalized_rows = project_turn_web_v2(turn);
    let active_row = if let Some((tcid, text)) = streaming_commentary_before_tool(turn) {
        Some(ActiveProjectedRow {
            kind: ASSISTANT_COMMENTARY.into(),
            text,
            before_tool_call_id: Some(tcid),
        })
    } else if let Some(text) = streaming_commentary_block_text(turn) {
        Some(ActiveProjectedRow {
            kind: ASSISTANT_COMMENTARY.into(),
            text,
            before_tool_call_id: None,
        })
    } else {
        streaming_open_answer_text(turn).map(|text| ActiveProjectedRow {
            kind: ASSISTANT_ANSWER.into(),
            text,
            before_tool_call_id: None,
        })
    };
    TurnProjection {
        finalized_rows,
        active_row,
    }
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
    use crate::cm_turn_layout::event::TurnEvent;
    use crate::cm_turn_layout::model::SegmentKind;
    use crate::cm_turn_layout::reduce::TurnReducer;

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
        // 终答由 overlay 承载，`project_turn` 不产生 `assistant_answer` 行。
        let rows = project_turn(&turn);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].kind, "tool");
        assert_eq!(rows[1].kind, "assistant_commentary");
        assert_eq!(rows[1].text, "工作区是空的。");
        assert_eq!(rows[1].tool_call_id.as_deref(), Some("tc_create"));
        assert_eq!(rows[2].kind, "tool");
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

    #[test]
    fn project_turn_web_v2_keeps_closed_commentary_rows_stable() {
        let turn = Turn {
            steps: vec![
                crate::cm_turn_layout::model::ToolStep {
                    tool_call_id: "tc_a".into(),
                    name: "list_tree".into(),
                    summary: "list".into(),
                    before_commentary: Some("第一段。".into()),
                },
                crate::cm_turn_layout::model::ToolStep {
                    tool_call_id: "tc_b".into(),
                    name: "read_file".into(),
                    summary: "read".into(),
                    before_commentary: Some("第二段。".into()),
                },
            ],
            ..Turn::default()
        };

        let first_projection = project_turn_web_v2(&Turn {
            steps: turn.steps[..1].to_vec(),
            ..Turn::default()
        });
        let full_projection = project_turn_web_v2(&turn);

        assert_eq!(first_projection, full_projection[..2]);
        assert_eq!(full_projection[0].kind, ASSISTANT_COMMENTARY);
        assert_eq!(full_projection[0].tool_call_id.as_deref(), Some("tc_a"));
        assert_eq!(full_projection[2].kind, ASSISTANT_COMMENTARY);
        assert_eq!(full_projection[2].tool_call_id.as_deref(), Some("tc_b"));
    }

    #[test]
    fn project_turn_projection_keeps_finalized_prefix_when_active_grows() {
        let mut turn = Turn {
            tool_phase_open: true,
            steps: vec![crate::cm_turn_layout::model::ToolStep {
                tool_call_id: "tc_a".into(),
                name: "list_tree".into(),
                summary: "list".into(),
                before_commentary: Some("已关闭旁白。".into()),
            }],
            segments: vec![crate::cm_turn_layout::model::TurnSegment {
                segment_id: "seg-open".into(),
                kind: SegmentKind::Commentary,
                before_tool_call_id: Some("tc_b".into()),
                text: "进行中。".into(),
                open: true,
            }],
            ..Turn::default()
        };

        let first = project_turn_projection(&turn);
        assert_eq!(first.finalized_rows.len(), 2);
        assert_eq!(
            first.active_row.as_ref().map(|a| a.text.as_str()),
            Some("进行中。")
        );
        assert_eq!(
            first
                .active_row
                .as_ref()
                .and_then(|a| a.before_tool_call_id.as_deref()),
            Some("tc_b")
        );

        turn.segments[0].text.push_str("续。");
        let second = project_turn_projection(&turn);
        assert_eq!(
            first.finalized_rows, second.finalized_rows,
            "active delta must not mutate finalized_rows"
        );
        assert_eq!(
            second.active_row.as_ref().map(|a| a.text.as_str()),
            Some("进行中。续。")
        );
    }

    #[test]
    fn project_turn_projection_active_open_answer() {
        let turn = Turn {
            segments: vec![crate::cm_turn_layout::model::TurnSegment {
                segment_id: "ans".into(),
                kind: SegmentKind::Answer,
                before_tool_call_id: None,
                text: "终答增量。".into(),
                open: true,
            }],
            ..Turn::default()
        };
        let proj = project_turn_projection(&turn);
        assert!(proj.finalized_rows.is_empty());
        let active = proj.active_row.expect("open answer");
        assert_eq!(active.kind, ASSISTANT_ANSWER);
        assert_eq!(active.text, "终答增量。");
        assert!(active.before_tool_call_id.is_none());
    }
}
