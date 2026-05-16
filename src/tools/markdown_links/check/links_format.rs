//! `markdown_check_links` 输出格式化（text / json / sarif）。

use std::path::Path;

use crate::tools::markdown_links::core::{
    LinkIssue, RULE_ANCHOR, RULE_EXTERNAL, RULE_LOCAL, RULE_ROOT, issue_json, issue_sarif,
};
use crate::tools::tool_param_types::MarkdownCheckLinksOutputFormat;

use super::TextReportSummary;
use super::render_text_report;

pub(super) struct MarkdownCheckScanStats {
    pub rel_ok: usize,
    pub fragment_ok: usize,
    pub fragment_bad: usize,
    pub external_checked_ok: usize,
    pub external_ignored: usize,
    pub special_skipped: usize,
    pub external_probe_requests: usize,
    pub external_cache_hits: usize,
}

pub(super) struct MarkdownCheckFormatInput<'a> {
    pub output_format: MarkdownCheckLinksOutputFormat,
    pub ws_canonical: &'a Path,
    pub roots: &'a [String],
    pub md_files_len: usize,
    pub allowed_prefixes: &'a [String],
    pub stats: MarkdownCheckScanStats,
    pub local_issues: &'a [LinkIssue],
    pub external_issues: &'a [LinkIssue],
}

pub(super) fn markdown_check_format_output(input: MarkdownCheckFormatInput<'_>) -> String {
    let MarkdownCheckFormatInput {
        output_format,
        ws_canonical,
        roots,
        md_files_len,
        allowed_prefixes,
        stats,
        local_issues,
        external_issues,
    } = input;
    let text = render_text_report(
        TextReportSummary {
            ws_canonical,
            roots,
            md_files_len,
            allowed_prefixes,
            rel_ok: stats.rel_ok,
            fragment_ok: stats.fragment_ok,
            fragment_bad: stats.fragment_bad,
            external_checked_ok: stats.external_checked_ok,
            external_ignored: stats.external_ignored,
            special_skipped: stats.special_skipped,
            external_probe_requests: stats.external_probe_requests,
            external_cache_hits: stats.external_cache_hits,
        },
        local_issues,
        external_issues,
    );
    let total_problems = local_issues.len() + external_issues.len();
    let all_issues = local_issues
        .iter()
        .chain(external_issues.iter())
        .map(issue_json)
        .collect::<Vec<_>>();
    match output_format {
        MarkdownCheckLinksOutputFormat::Text => text,
        MarkdownCheckLinksOutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
            "tool": "markdown_check_links",
            "workspace": ws_canonical.to_string_lossy(),
            "roots": roots,
            "summary": {
                "markdown_files_scanned": md_files_len,
                "relative_ok": stats.rel_ok,
                "local_issues": local_issues.len(),
                "fragment_ok": stats.fragment_ok,
                "fragment_issues": stats.fragment_bad,
                "external_checked_ok": stats.external_checked_ok,
                "external_issues": external_issues.len(),
                "external_ignored": stats.external_ignored,
                "special_skipped": stats.special_skipped,
                "external_probe_requests": stats.external_probe_requests,
                "external_cache_hits": stats.external_cache_hits,
                "problems": total_problems
            },
            "problems": all_issues
        }))
        .unwrap_or_else(|e| format!("JSON 序列化失败: {}", e)),
        MarkdownCheckLinksOutputFormat::Sarif => {
            let results = local_issues
                .iter()
                .chain(external_issues.iter())
                .map(issue_sarif)
                .collect::<Vec<_>>();
            serde_json::to_string_pretty(&serde_json::json!({
                "version": "2.1.0",
                "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
                "runs": [{
                    "tool": {
                        "driver": {
                            "name": "markdown_check_links",
                            "rules": [
                                { "id": RULE_LOCAL, "shortDescription": { "text": "Markdown 相对路径问题" } },
                                { "id": RULE_ANCHOR, "shortDescription": { "text": "Markdown 锚点问题" } },
                                { "id": RULE_EXTERNAL, "shortDescription": { "text": "Markdown 外链探测失败" } },
                                { "id": RULE_ROOT, "shortDescription": { "text": "Markdown 扫描根路径问题" } }
                            ]
                        }
                    },
                    "results": results,
                    "invocations": [{
                        "executionSuccessful": true,
                        "toolExecutionNotifications": [{
                            "message": { "text": text }
                        }]
                    }]
                }]
            }))
            .unwrap_or_else(|e| format!("SARIF 序列化失败: {}", e))
        }
    }
}
