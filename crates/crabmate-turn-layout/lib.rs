//! Canonical **`Turn`** layout + reducer + projector（对齐 OpenAI assistant→tool→assistant 与 AG-UI 段边界）。
//!
//! - **Reducer**：按 SSE / 内部事件更新 canonical 状态（允许事件到达顺序与展示顺序不同）。
//! - **Projector**：`Turn` → 有序 [`ProjectedRow`]；`project_turn` 为逐步旁注金样，`project_turn_web` 为 Web 块布局。
//!
//! 金样：`fixtures/turn_project_golden.jsonl`（逐步 `project_turn`）、`fixtures/turn_project_web_golden.jsonl`（Web 块布局 `project_turn_web`）；
//! 测试：`cargo test -p crabmate-turn-layout golden_turn_project` / `golden_turn_project_web`。

mod event;
mod model;
mod project;
mod reduce;

pub use event::TurnEvent;
pub use model::{PENDING_STREAM_COMMENTARY_SEGMENT_ID, SegmentKind, Turn, TurnSegment};
pub use project::{
    ASSISTANT_ANSWER, ASSISTANT_BATCH_NARRATION, ProjectedRow, batch_narration_row,
    batch_narration_text, commentary_for_tool, project_turn, project_turn_web,
    streaming_commentary_block_text,
};
pub use reduce::{TurnReducer, reduce_event};

#[cfg(test)]
mod golden {
    use std::fs;
    use std::path::{Path, PathBuf};

    use serde::Deserialize;

    use crate::event::TurnEvent;
    use crate::model::Turn;
    use crate::project::{ProjectedRow, project_turn, project_turn_web};
    use crate::reduce::TurnReducer;

    #[derive(Debug, Deserialize)]
    struct GoldenCase {
        id: String,
        events: Vec<TurnEvent>,
        expect: Vec<ProjectedRow>,
        #[serde(default)]
        expect_open_preview: Option<String>,
    }

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
    }

    fn load_cases(path: &Path) -> Vec<(usize, GoldenCase)> {
        let raw =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        raw.lines()
            .enumerate()
            .filter_map(|(line_no, line)| {
                let t = line.trim();
                if t.is_empty() || t.starts_with('#') {
                    return None;
                }
                let case: GoldenCase = serde_json::from_str(t).unwrap_or_else(|e| {
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

    fn reduce_events(events: Vec<TurnEvent>) -> Turn {
        let mut turn = Turn::default();
        let reducer = TurnReducer;
        for ev in events {
            reducer.apply(&mut turn, ev);
        }
        turn
    }

    #[test]
    fn golden_turn_project() {
        let path = fixture_path("turn_project_golden.jsonl");
        for (line_no, case) in load_cases(&path) {
            let turn = reduce_events(case.events);
            let got = project_turn(&turn);
            assert_eq!(
                got,
                case.expect,
                "case {} at {}:{}",
                case.id,
                path.display(),
                line_no
            );
        }
    }

    #[test]
    fn golden_turn_project_web() {
        let path = fixture_path("turn_project_web_golden.jsonl");
        for (line_no, case) in load_cases(&path) {
            let turn = reduce_events(case.events);
            let got = project_turn_web(&turn);
            assert_eq!(
                got,
                case.expect,
                "case {} at {}:{}",
                case.id,
                path.display(),
                line_no
            );
            if let Some(ref preview) = case.expect_open_preview {
                let open = crate::streaming_commentary_block_text(&turn).unwrap_or_default();
                assert_eq!(
                    open,
                    *preview,
                    "case {} open preview at {}:{}",
                    case.id,
                    path.display(),
                    line_no
                );
                if let Some(batch) = crate::batch_narration_text(&turn) {
                    assert!(
                        !batch.contains(preview.as_str()),
                        "case {}: open preview must not duplicate batch row",
                        case.id
                    );
                }
            }
        }
    }
}
