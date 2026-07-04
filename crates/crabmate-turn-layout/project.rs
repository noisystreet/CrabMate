use crate::model::Turn;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProjectedRow {
    /// `assistant_timeline` | `assistant_commentary` | `assistant_answer` | `tool`
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

fn row(kind: &str, text: impl Into<String>) -> ProjectedRow {
    ProjectedRow {
        kind: kind.to_string(),
        text: text.into(),
        tool_name: None,
        tool_call_id: None,
    }
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
        out.push(row("assistant_answer", a.clone()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::TurnEvent;
    use crate::model::SegmentKind;
    use crate::reduce::TurnReducer;

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
}
