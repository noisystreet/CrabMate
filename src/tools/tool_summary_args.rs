//! Typed tool-argument shapes for **dynamic** chat summaries (`ToolSummaryKind::Dynamic`).
//! Field names match tool JSON schemas; extra keys are ignored (no `deny_unknown_fields`).
//! Parsing uses `serde_json::from_value`; on failure the caller yields no summary (same as
//! missing required fields in the previous `Value::get` style).

use serde::Deserialize;

/// Deserialize from a clone of `v`, then build the summary line.
pub(super) fn summarize_from_value<T>(v: &serde_json::Value) -> Option<String>
where
    T: serde::de::DeserializeOwned + ToolSummaryLine,
{
    let t: T = serde_json::from_value(v.clone()).ok()?;
    t.summary_line()
}

pub(super) trait ToolSummaryLine {
    fn summary_line(self) -> Option<String>;
}

include!("tool_summary_args/fragment_core.rs");
include!("tool_summary_args/fragment_git_files.rs");
include!("tool_summary_args/fragment_gh_archive.rs");
