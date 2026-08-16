//! Canonical **`Turn`** layout + reducer + projector（对齐 OpenAI assistant→tool→assistant 与 AG-UI 段边界）。
//!
//! - **Reducer**：按 SSE / 内部事件更新 canonical 状态（允许事件到达顺序与展示顺序不同）。
//! - **Projector**：`Turn` → 有序 [`ProjectedRow`]；`project_turn_web` 保留 v1 块布局，
//!   `project_turn_web_v2` / [`project_turn_projection`] 为 Web 定稿行 + 可选 active。
//!
//! 金样：`fixtures/turn_project_golden.jsonl`（逐步 `project_turn`）、`fixtures/turn_project_web_golden.jsonl`（Web 块布局 `project_turn_web`）、
//! `fixtures/turn_project_projection_golden.jsonl`（`project_turn_projection` finalized/active）；
//! 测试：`cargo test --lib golden_turn_project` / `golden_turn_project_web` / `golden_turn_project_projection`。

mod event;
mod model;
mod project;
mod reduce;
pub mod replay;

pub use event::TurnEvent;
pub use model::{PENDING_STREAM_COMMENTARY_SEGMENT_ID, SegmentKind, Turn, TurnSegment};
pub use project::{
    ASSISTANT_ANSWER, ASSISTANT_BATCH_NARRATION, ASSISTANT_COMMENTARY, ActiveProjectedRow,
    ProjectedRow, TurnProjection, batch_narration_row, batch_narration_text, commentary_for_tool,
    project_turn, project_turn_projection, project_turn_web, project_turn_web_v2,
    streaming_commentary_before_tool, streaming_commentary_block_text,
};
pub use reduce::{
    TurnReducer, close_open_commentary_segments, reduce_event, try_split_combined_post_tool_answer,
};

#[cfg(test)]
mod golden {
    use std::fs;
    use std::path::{Path, PathBuf};

    use serde::Deserialize;

    use crate::cm_turn_layout::event::TurnEvent;
    use crate::cm_turn_layout::model::Turn;
    use crate::cm_turn_layout::project::{
        ActiveProjectedRow, ProjectedRow, project_turn, project_turn_projection, project_turn_web,
    };
    use crate::cm_turn_layout::reduce::TurnReducer;

    #[derive(Debug, Deserialize)]
    struct GoldenCase {
        id: String,
        events: Vec<TurnEvent>,
        expect: Vec<ProjectedRow>,
        #[serde(default)]
        expect_open_preview: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct ProjectionGoldenCase {
        id: String,
        events: Vec<TurnEvent>,
        expect_finalized: Vec<ProjectedRow>,
        #[serde(default)]
        expect_active: Option<ActiveProjectedRow>,
    }

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name)
    }

    fn load_cases<T: for<'de> Deserialize<'de>>(path: &Path) -> Vec<(usize, T)> {
        let raw =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        raw.lines()
            .enumerate()
            .filter_map(|(line_no, line)| {
                let t = line.trim();
                if t.is_empty() || t.starts_with('#') {
                    return None;
                }
                let case: T = serde_json::from_str(t).unwrap_or_else(|e| {
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
        for (line_no, case) in load_cases::<GoldenCase>(&path) {
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
        for (line_no, case) in load_cases::<GoldenCase>(&path) {
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
                let open = crate::cm_turn_layout::streaming_commentary_block_text(&turn)
                    .unwrap_or_default();
                assert_eq!(
                    open,
                    *preview,
                    "case {} open preview at {}:{}",
                    case.id,
                    path.display(),
                    line_no
                );
                if let Some(batch) = crate::cm_turn_layout::batch_narration_text(&turn) {
                    assert!(
                        !batch.contains(preview.as_str()),
                        "case {}: open preview must not duplicate batch row",
                        case.id
                    );
                }
            }
        }
    }

    #[test]
    fn golden_turn_project_projection() {
        let path = fixture_path("turn_project_projection_golden.jsonl");
        for (line_no, case) in load_cases::<ProjectionGoldenCase>(&path) {
            let turn = reduce_events(case.events.clone());
            let got = project_turn_projection(&turn);
            assert_eq!(
                got.finalized_rows,
                case.expect_finalized,
                "case {} finalized at {}:{}",
                case.id,
                path.display(),
                line_no
            );
            assert_eq!(
                got.active_row,
                case.expect_active,
                "case {} active at {}:{}",
                case.id,
                path.display(),
                line_no
            );
            // active 增长不得改写已定稿前缀：逐步重放，每次 finalized 须为最终 finalized 的前缀。
            let mut prefix_turn = Turn::default();
            let reducer = TurnReducer;
            let mut last_finalized_len = 0usize;
            for ev in case.events {
                reducer.apply(&mut prefix_turn, ev);
                let snap = project_turn_projection(&prefix_turn);
                assert!(
                    snap.finalized_rows.len() >= last_finalized_len,
                    "case {}: finalized_rows must be monotonic",
                    case.id
                );
                assert_eq!(
                    &got.finalized_rows[..snap.finalized_rows.len()],
                    snap.finalized_rows.as_slice(),
                    "case {}: finalized prefix must be stable while events arrive",
                    case.id
                );
                last_finalized_len = snap.finalized_rows.len();
            }
        }
    }
}
