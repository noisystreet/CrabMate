//! 扫描 Markdown 文件并汇总链接校验结果（从 `check/mod.rs` 拆出以降低单文件行数）。

use std::path::Path;

use crate::cm_tools::tool_result::ToolError;

use super::MarkdownCheckParsed;
use super::links_collect::markdown_check_collect_files;
use super::links_format::markdown_check_format_output;
use super::links_scan::{MarkdownCheckScanState, markdown_check_scan_all_files};

/// 扫描并汇总；工作区根 / 子路径解析失败走 `markdown_check_links_workspace_failed`，
/// 扫描期硬错误（如 HTTP 客户端未初始化）走 `markdown_check_links_failed`。
#[allow(clippy::result_large_err)]
pub(super) fn markdown_check_links_inner_try(
    parsed: MarkdownCheckParsed,
    working_dir: &Path,
) -> Result<String, ToolError> {
    let collected = markdown_check_collect_files(&parsed, working_dir).map_err(|e| {
        ToolError::external_code("markdown_check_links_workspace_failed", format!("错误：{e}"))
    })?;

    let mut state = MarkdownCheckScanState::new();
    state.local_issues.extend(collected.root_issues);

    if let Some(err) = markdown_check_scan_all_files(
        &parsed,
        &collected.ws_canonical,
        &collected.md_files,
        &collected.http_client,
        &mut state,
    ) {
        return Err(ToolError::external_code("markdown_check_links_failed", err));
    }

    Ok(markdown_check_format_output(
        super::links_format::MarkdownCheckFormatInput {
            output_format: parsed.output_format,
            ws_canonical: &collected.ws_canonical,
            roots: &parsed.roots,
            md_files_len: collected.md_files.len(),
            allowed_prefixes: &parsed.allowed_prefixes,
            stats: state.stats,
            local_issues: &state.local_issues,
            external_issues: &state.external_issues,
        },
    ))
}
