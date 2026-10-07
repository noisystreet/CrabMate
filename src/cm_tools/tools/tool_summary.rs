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
    summary_search_in_files => SearchInFilesSummaryArgs,
    summary_run_command => RunCommandSummaryArgs,
    summary_terminal_session => TerminalSessionSummaryArgs,
    summary_rust_analyzer_goto_definition => RustAnalyzerGotoDefSummaryArgs,
    summary_rust_analyzer_find_references => RustAnalyzerFindRefsSummaryArgs,
    summary_rust_analyzer_hover => RustAnalyzerHoverSummaryArgs,
    summary_rust_analyzer_document_symbol => RustAnalyzerDocSymbolSummaryArgs,
    summary_rust_analyzer_goto_implementation => RustAnalyzerGotoImplSummaryArgs,
    summary_rust_analyzer_goto_type_definition => RustAnalyzerGotoTypeDefSummaryArgs,
    summary_rust_analyzer_document_highlight => RustAnalyzerDocHighlightSummaryArgs,
    summary_rust_analyzer_workspace_symbol => RustAnalyzerWorkspaceSymbolSummaryArgs,
    summary_uv_run => UvRunSummaryArgs,
    summary_python_snippet_run => PythonSnippetRunSummaryArgs,
    summary_error_output_playbook => ErrorOutputPlaybookSummaryArgs,
    summary_pre_commit_run => PreCommitRunSummaryArgs,
    summary_ast_grep_run => AstGrepRunSummaryArgs,
    summary_ast_grep_rewrite => AstGrepRewriteSummaryArgs,
    summary_git_diff => GitDiffSummaryArgs,
    summary_create_file => CreateFileSummaryArgs,
    summary_modify_file => ModifyFileSummaryArgs,
    summary_copy_file => CopyFileSummaryArgs,
    summary_move_file => MoveFileSummaryArgs,
    summary_read_file => ReadFileSummaryArgs,
    summary_read_dir => ReadDirSummaryArgs,
    summary_web_search => WebSearchSummaryArgs,
    summary_http_fetch => HttpFetchSummaryArgs,
    summary_http_request => HttpRequestSummaryArgs,
    summary_glob_files => GlobFilesSummaryArgs,
    summary_markdown_check_links => MarkdownCheckLinksSummaryArgs,
    summary_structured_validate => StructuredValidateSummaryArgs,
    summary_structured_query => StructuredQuerySummaryArgs,
    summary_structured_diff => StructuredDiffSummaryArgs,
    summary_structured_patch => StructuredPatchSummaryArgs,
    summary_list_tree => ListTreeSummaryArgs,
    summary_file_exists => FileExistsSummaryArgs,
    summary_read_binary_meta => ReadBinaryMetaSummaryArgs,
    summary_hash_file => HashFileSummaryArgs,
    summary_extract_in_file => ExtractInFileSummaryArgs,
    summary_apply_patch => ApplyPatchSummaryArgs,
    summary_package_query => PackageQuerySummaryArgs,
    summary_find_symbol => FindSymbolSummaryArgs,
    summary_find_references => FindReferencesSummaryArgs,
    summary_call_graph_sketch => CallGraphSketchSummaryArgs,
    summary_rust_file_outline => RustFileOutlineSummaryArgs,
    summary_format_check_file => FormatCheckFileSummaryArgs,
    summary_convert_units => ConvertUnitsSummaryArgs,
    summary_port_check => PortCheckSummaryArgs,
    summary_process_list => ProcessListSummaryArgs,
    summary_background_job_status => BackgroundJobStatusSummaryArgs,
    summary_background_job_list => BackgroundJobListSummaryArgs,
    summary_background_job_output => BackgroundJobOutputSummaryArgs,
    summary_background_job_cancel => BackgroundJobCancelSummaryArgs,
    summary_code_stats => CodeStatsSummaryArgs,
    summary_dependency_graph => DependencyGraphSummaryArgs,
    summary_coverage_report => CoverageReportSummaryArgs,
    summary_delete_files => DeleteFilesSummaryArgs,
    summary_delete_dir => DeleteDirSummaryArgs,
    summary_append_file => AppendFileSummaryArgs,
    summary_create_dir => CreateDirSummaryArgs,
    summary_search_replace => SearchReplaceSummaryArgs,
    summary_chmod_file => ChmodFileSummaryArgs,
    summary_symlink_info => SymlinkInfoSummaryArgs,
    summary_gh_pr_checks => GhPrChecksSummaryArgs,
    summary_gh_pr_create => GhPrCreateSummaryArgs,
    summary_gh_pr_merge => GhPrMergeSummaryArgs,
    summary_gh_pr_review => GhPrReviewSummaryArgs,
    summary_gh_pr_comment => GhPrCommentSummaryArgs,
    summary_gh_pr_body_draft => GhPrBodyDraftSummaryArgs,
    summary_gh_pr_edit => GhPrEditSummaryArgs,
    summary_gh_issue_create => GhIssueCreateSummaryArgs,
    summary_gh_run_rerun => GhRunRerunSummaryArgs,
    summary_gh_run_failure_summary => GhRunFailureSummarySummaryArgs,
    summary_gh_release_create => GhReleaseCreateSummaryArgs,
    summary_gh_api => GhApiSummaryArgs,
    summary_archive_pack => ArchivePackSummaryArgs,
    summary_archive_unpack => ArchiveUnpackSummaryArgs,
    summary_archive_list => ArchiveListSummaryArgs,
}
