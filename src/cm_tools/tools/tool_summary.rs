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
    summary_python_snippet_run => PythonSnippetRunSummaryArgs,
    summary_git => GitSummaryArgs,
    summary_modify_file => ModifyFileSummaryArgs,
    summary_copy_file => CopyFileSummaryArgs,
    summary_move_file => MoveFileSummaryArgs,
    summary_read_dir => ReadDirSummaryArgs,
    summary_web_search => WebSearchSummaryArgs,
    summary_glob_files => GlobFilesSummaryArgs,
    summary_markdown_check_links => MarkdownCheckLinksSummaryArgs,
    summary_list_tree => ListTreeSummaryArgs,
    summary_apply_patch => ApplyPatchSummaryArgs,
    summary_code_nav => CodeNavSummaryArgs,
    summary_gh_pr => GhPrSummaryArgs,
    summary_gh_run => GhRunSummaryArgs,
    summary_archive_pack => ArchivePackSummaryArgs,
    summary_archive_unpack => ArchiveUnpackSummaryArgs,
    summary_archive_list => ArchiveListSummaryArgs,
}
