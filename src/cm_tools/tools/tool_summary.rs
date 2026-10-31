//! Dynamic summary helpers for `ToolSpec::summary` `Dynamic` variants.
//! Argument shapes live in [`super::tool_summary_args`] (`serde` structs + [`ToolSummaryLine`]).

use super::tool_summary_args::*;

/// 生成 `summary_*` 壳函数：每个都只是把 `&serde_json::Value` 交给
/// [`summarize_from_value`] 并以对应 args 类型反序列化。
///
/// 新增动态摘要工具时，只需在下表增一行 `summary_x => XxxSummaryArgs`（args 结构体与
/// `ToolSummaryLine` 实现按需加到 `tool_summary_args/` 的 fragment 中）。
macro_rules! define_summaries {
    ($( $name:ident => $args:ty ),* $(,)?) => {
        $(
            pub(super) fn $name(v: &serde_json::Value) -> Option<String> {
                summarize_from_value::<$args>(v)
            }
        )*
    };
}

define_summaries! {
    summary_codebase_semantic_search => CodebaseSemanticSearchSummaryArgs,
    summary_rust_analyzer => RustAnalyzerSummaryArgs,
    summary_gh_pr => GhPrSummaryArgs,
    summary_gh_run => GhRunSummaryArgs,
}

/// `copy_file` / `move_file` 共用 `FileFromToOverwriteArgs`：字段一致、只有动词不同，
/// 摘要文本无法由 Args 类型单独决定，所以走 `specs` 的 `summary: dynamic by <fn>`
/// 而不是 `define_summaries!`。实现仍与 Args 同侧，见 `tool_summary_args`。
pub(super) use super::tool_summary_args::{summary_copy_file, summary_move_file};
