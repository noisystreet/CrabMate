//! Dynamic summary helpers for `ToolSpec::summary` `Dynamic` variants.
//! Argument shapes live in [`super::tool_summary_args`] (`serde` structs + [`ToolSummaryLine`]).

use super::tool_summary_args::*;

pub(super) fn summary_codebase_semantic_search(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<CodebaseSemanticSearchSummaryArgs>(v)
}

pub(super) fn summary_search_in_files(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<SearchInFilesSummaryArgs>(v)
}

pub(super) fn summary_run_command(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<RunCommandSummaryArgs>(v)
}

pub(super) fn summary_rust_analyzer_goto_definition(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<RustAnalyzerGotoDefSummaryArgs>(v)
}

pub(super) fn summary_rust_analyzer_find_references(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<RustAnalyzerFindRefsSummaryArgs>(v)
}

pub(super) fn summary_rust_analyzer_hover(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<RustAnalyzerHoverSummaryArgs>(v)
}

pub(super) fn summary_rust_analyzer_document_symbol(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<RustAnalyzerDocSymbolSummaryArgs>(v)
}

pub(super) fn summary_python_install_editable(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<PythonInstallEditableSummaryArgs>(v)
}

pub(super) fn summary_uv_run(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<UvRunSummaryArgs>(v)
}

pub(super) fn summary_python_snippet_run(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<PythonSnippetRunSummaryArgs>(v)
}

pub(super) fn summary_error_output_playbook(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<ErrorOutputPlaybookSummaryArgs>(v)
}

pub(super) fn summary_pre_commit_run(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<PreCommitRunSummaryArgs>(v)
}

pub(super) fn summary_ast_grep_run(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<AstGrepRunSummaryArgs>(v)
}

pub(super) fn summary_ast_grep_rewrite(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<AstGrepRewriteSummaryArgs>(v)
}

pub(super) fn summary_git_diff(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GitDiffSummaryArgs>(v)
}

pub(super) fn summary_git_diff_stat(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GitDiffStatSummaryArgs>(v)
}

pub(super) fn summary_git_diff_names(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GitDiffNamesSummaryArgs>(v)
}

pub(super) fn summary_create_file(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<CreateFileSummaryArgs>(v)
}

pub(super) fn summary_modify_file(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<ModifyFileSummaryArgs>(v)
}

pub(super) fn summary_copy_file(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<CopyFileSummaryArgs>(v)
}

pub(super) fn summary_move_file(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<MoveFileSummaryArgs>(v)
}

pub(super) fn summary_read_file(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<ReadFileSummaryArgs>(v)
}

pub(super) fn summary_read_dir(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<ReadDirSummaryArgs>(v)
}

pub(super) fn summary_web_search(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<WebSearchSummaryArgs>(v)
}

pub(super) fn summary_http_fetch(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<HttpFetchSummaryArgs>(v)
}

pub(super) fn summary_http_request(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<HttpRequestSummaryArgs>(v)
}

pub(super) fn summary_glob_files(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GlobFilesSummaryArgs>(v)
}

pub(super) fn summary_markdown_check_links(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<MarkdownCheckLinksSummaryArgs>(v)
}

pub(super) fn summary_structured_validate(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<StructuredValidateSummaryArgs>(v)
}

pub(super) fn summary_structured_query(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<StructuredQuerySummaryArgs>(v)
}

pub(super) fn summary_structured_diff(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<StructuredDiffSummaryArgs>(v)
}

pub(super) fn summary_structured_patch(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<StructuredPatchSummaryArgs>(v)
}

pub(super) fn summary_list_tree(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<ListTreeSummaryArgs>(v)
}

pub(super) fn summary_file_exists(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<FileExistsSummaryArgs>(v)
}

pub(super) fn summary_read_binary_meta(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<ReadBinaryMetaSummaryArgs>(v)
}

pub(super) fn summary_hash_file(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<HashFileSummaryArgs>(v)
}

pub(super) fn summary_extract_in_file(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<ExtractInFileSummaryArgs>(v)
}

pub(super) fn summary_apply_patch(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<ApplyPatchSummaryArgs>(v)
}

pub(super) fn summary_package_query(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<PackageQuerySummaryArgs>(v)
}

pub(super) fn summary_find_symbol(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<FindSymbolSummaryArgs>(v)
}

pub(super) fn summary_find_references(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<FindReferencesSummaryArgs>(v)
}

pub(super) fn summary_call_graph_sketch(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<CallGraphSketchSummaryArgs>(v)
}

pub(super) fn summary_rust_file_outline(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<RustFileOutlineSummaryArgs>(v)
}

pub(super) fn summary_format_check_file(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<FormatCheckFileSummaryArgs>(v)
}

pub(super) fn summary_convert_units(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<ConvertUnitsSummaryArgs>(v)
}

pub(super) fn summary_git_checkout(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GitCheckoutSummaryArgs>(v)
}

pub(super) fn summary_git_branch_create(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GitBranchCreateSummaryArgs>(v)
}

pub(super) fn summary_git_branch_delete(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GitBranchDeleteSummaryArgs>(v)
}

pub(super) fn summary_git_push(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GitPushSummaryArgs>(v)
}

pub(super) fn summary_git_merge(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GitMergeSummaryArgs>(v)
}

pub(super) fn summary_git_rebase(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GitRebaseSummaryArgs>(v)
}

pub(super) fn summary_git_stash(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GitStashSummaryArgs>(v)
}

pub(super) fn summary_git_tag(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GitTagSummaryArgs>(v)
}

pub(super) fn summary_git_reset(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GitResetSummaryArgs>(v)
}

pub(super) fn summary_git_revert(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GitRevertSummaryArgs>(v)
}

pub(super) fn summary_npm_run(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<NpmRunSummaryArgs>(v)
}

pub(super) fn summary_npx_run(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<NpxRunSummaryArgs>(v)
}

pub(super) fn summary_port_check(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<PortCheckSummaryArgs>(v)
}

pub(super) fn summary_process_list(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<ProcessListSummaryArgs>(v)
}

pub(super) fn summary_code_stats(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<CodeStatsSummaryArgs>(v)
}

pub(super) fn summary_dependency_graph(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<DependencyGraphSummaryArgs>(v)
}

pub(super) fn summary_coverage_report(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<CoverageReportSummaryArgs>(v)
}

pub(super) fn summary_delete_file(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<DeleteFileSummaryArgs>(v)
}

pub(super) fn summary_delete_dir(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<DeleteDirSummaryArgs>(v)
}

pub(super) fn summary_append_file(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<AppendFileSummaryArgs>(v)
}

pub(super) fn summary_create_dir(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<CreateDirSummaryArgs>(v)
}

pub(super) fn summary_search_replace(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<SearchReplaceSummaryArgs>(v)
}

pub(super) fn summary_chmod_file(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<ChmodFileSummaryArgs>(v)
}

pub(super) fn summary_symlink_info(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<SymlinkInfoSummaryArgs>(v)
}

pub(super) fn summary_gh_pr_list(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GhPrListSummaryArgs>(v)
}

pub(super) fn summary_gh_pr_view(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GhPrNumberSummaryArgs>(v)
}

pub(super) fn summary_gh_issue_list(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GhIssueListSummaryArgs>(v)
}

pub(super) fn summary_gh_issue_view(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GhIssueViewSummaryArgs>(v)
}

pub(super) fn summary_gh_run_list(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GhRunListSummaryArgs>(v)
}

pub(super) fn summary_gh_pr_diff(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GhPrDiffSummaryArgs>(v)
}

pub(super) fn summary_gh_run_view(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GhRunViewSummaryArgs>(v)
}

pub(super) fn summary_gh_release_list(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GhReleaseListSummaryArgs>(v)
}

pub(super) fn summary_gh_release_view(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GhReleaseViewSummaryArgs>(v)
}

pub(super) fn summary_gh_search(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GhSearchSummaryArgs>(v)
}

pub(super) fn summary_gh_api(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<GhApiSummaryArgs>(v)
}

pub(super) fn summary_archive_pack(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<ArchivePackSummaryArgs>(v)
}

pub(super) fn summary_archive_unpack(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<ArchiveUnpackSummaryArgs>(v)
}

pub(super) fn summary_archive_list(v: &serde_json::Value) -> Option<String> {
    summarize_from_value::<ArchiveListSummaryArgs>(v)
}
