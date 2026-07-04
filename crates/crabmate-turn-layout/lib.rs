//! Canonical **`Turn`** layout + reducer + projector（对齐 OpenAI assistant→tool→assistant 与 AG-UI 段边界）。
//!
//! - **Reducer**：按 SSE / 内部事件更新 canonical 状态（允许事件到达顺序与展示顺序不同）。
//! - **Projector**：`Turn` → 有序 [`ProjectedRow`]，供 Web/TUI/导出金样共用。
//!
//! 金样：`fixtures/turn_project_golden.jsonl`；测试：`cargo test -p crabmate-turn-layout golden_turn_project`。

mod event;
mod model;
mod project;
mod reduce;

pub use event::TurnEvent;
pub use model::{PENDING_STREAM_COMMENTARY_SEGMENT_ID, SegmentKind, Turn, TurnSegment};
pub use project::{ProjectedRow, project_turn};
pub use reduce::{TurnReducer, reduce_event};

#[cfg(test)]
mod golden {
    use std::fs;
    use std::path::PathBuf;

    use serde::Deserialize;

    use crate::event::TurnEvent;
    use crate::model::Turn;
    use crate::project::{ProjectedRow, project_turn};
    use crate::reduce::TurnReducer;

    #[derive(Debug, Deserialize)]
    struct GoldenCase {
        id: String,
        events: Vec<TurnEvent>,
        expect: Vec<ProjectedRow>,
    }

    #[test]
    fn golden_turn_project() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("../../fixtures/turn_project_golden.jsonl");
        let raw =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for (line_no, line) in raw.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let case: GoldenCase = serde_json::from_str(t).unwrap_or_else(|e| {
                panic!(
                    "{}:{}: invalid golden json: {e}\n{t}",
                    path.display(),
                    line_no + 1
                );
            });
            let mut turn = Turn::default();
            let reducer = TurnReducer;
            for ev in case.events {
                reducer.apply(&mut turn, ev);
            }
            let got = project_turn(&turn);
            assert_eq!(
                got,
                case.expect,
                "case {} at {}:{}",
                case.id,
                path.display(),
                line_no + 1
            );
        }
    }
}
