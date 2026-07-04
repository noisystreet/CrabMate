//! Web 块布局契约：`project_turn_web` → `BubbleOutputQueue` flush → `StoredMessage` 顺序。

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crabmate_turn_layout::{SegmentKind, TurnEvent, project_turn_web};
    use serde::Deserialize;

    use super::super::super::super::turn_canonical::TurnCanonicalState;
    use crate::sse_dispatch::TurnSegmentStartInfo;
    use crate::storage::StoredMessage;

    use super::super::bubble_queue::{BATCH_NARRATION_ROW_ID, BubbleOutputQueue};

    #[derive(Debug, Deserialize)]
    struct WebGoldenCase {
        id: String,
        events: Vec<TurnEvent>,
        expect: Vec<crabmate_turn_layout::ProjectedRow>,
        #[serde(default)]
        expect_open_preview: Option<String>,
    }

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/turn_project_web_golden.jsonl")
    }

    fn load_cases() -> Vec<(usize, WebGoldenCase)> {
        let path = fixture_path();
        let raw =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        raw.lines()
            .enumerate()
            .filter_map(|(line_no, line)| {
                let t = line.trim();
                if t.is_empty() || t.starts_with('#') {
                    return None;
                }
                let case: WebGoldenCase = serde_json::from_str(t).unwrap_or_else(|e| {
                    panic!(
                        "{}:{}: invalid golden json: {e}\n{t}",
                        path.display(),
                        line_no + 1
                    );
                });
                Some((line_no + 1, case))
            })
            .collect()
    }

    fn apply_event(turn: &mut TurnCanonicalState, ev: TurnEvent) {
        match ev {
            TurnEvent::TimelineAssistant { text } => {
                turn.ingest_pre_tool_commentary(text.as_str());
            }
            TurnEvent::SegmentStart {
                segment_id,
                kind,
                before_tool_call_id,
            } => {
                turn.on_segment_start(TurnSegmentStartInfo {
                    segment_id,
                    kind: match kind {
                        SegmentKind::Commentary => "commentary".to_string(),
                        SegmentKind::Answer => "answer".to_string(),
                    },
                    before_tool_call_id,
                });
            }
            TurnEvent::SegmentDelta {
                segment_id: _,
                delta,
            } => {
                let _ = turn.try_apply_commentary_delta(delta.as_str());
            }
            TurnEvent::SegmentEnd { segment_id } => {
                turn.on_segment_end(segment_id);
            }
            TurnEvent::ToolCall {
                tool_call_id,
                name,
                summary,
            } => {
                turn.on_tool_call(tool_call_id.as_str(), name.as_str(), summary.as_str());
            }
            TurnEvent::ToolPhaseEnd => {
                turn.on_tool_phase_end();
            }
            TurnEvent::AnswerDelta { delta } => {
                let _ = turn.try_apply_answer_delta(delta.as_str());
            }
        }
    }

    fn tool_messages_from_projection(turn: &TurnCanonicalState) -> Vec<StoredMessage> {
        project_turn_web(turn.turn_ref())
            .into_iter()
            .filter(|r| r.kind == "tool")
            .map(|r| StoredMessage {
                id: format!("tool-{}", r.tool_call_id.clone().unwrap_or_default()),
                role: "system".into(),
                text: r.text,
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: true,
                tool_call_id: r.tool_call_id,
                tool_name: r.tool_name,
                created_at: 0,
            })
            .collect()
    }

    fn batch_row_index(messages: &[StoredMessage]) -> Option<usize> {
        messages.iter().position(|m| m.id == BATCH_NARRATION_ROW_ID)
    }

    #[test]
    fn golden_turn_web_stored_sync() {
        let path = fixture_path();
        for (line_no, case) in load_cases() {
            let mut turn = TurnCanonicalState::new();
            for ev in case.events {
                apply_event(&mut turn, ev);
            }
            assert_eq!(
                project_turn_web(turn.turn_ref()),
                case.expect,
                "projection drift in case {} at {}:{}",
                case.id,
                path.display(),
                line_no
            );

            let mut messages = tool_messages_from_projection(&turn);
            BubbleOutputQueue.flush_batch_narration_row(&mut messages, &turn);

            let batch = crabmate_turn_layout::batch_narration_row(turn.turn_ref())
                .expect("case must define batch row when tools exist");
            let batch_idx = batch_row_index(&messages).unwrap_or_else(|| {
                panic!(
                    "case {} at {}:{}: missing turn-batch-narration row",
                    case.id,
                    path.display(),
                    line_no
                )
            });
            assert_eq!(
                messages[batch_idx].text, batch.text,
                "case {} batch text",
                case.id
            );

            if let Some(ref anchor) = batch.tool_call_id {
                let tool_idx = messages
                    .iter()
                    .position(|m| m.is_tool && m.tool_call_id.as_deref() == Some(anchor.as_str()))
                    .unwrap_or_else(|| panic!("case {} missing anchor tool {anchor}", case.id));
                assert!(
                    batch_idx < tool_idx,
                    "case {}: batch row must precede anchor tool",
                    case.id
                );
            }

            if let Some(ref preview) = case.expect_open_preview {
                assert_eq!(
                    BubbleOutputQueue::loading_preview_text(&turn),
                    preview.as_str(),
                    "case {} open preview",
                    case.id
                );
            }
        }
    }
}
