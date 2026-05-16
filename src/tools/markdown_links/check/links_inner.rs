//! 扫描 Markdown 文件并汇总链接校验结果（从 `check/mod.rs` 拆出以降低单文件行数）。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::tools::tool_param_types::MarkdownCheckLinksOutputFormat;
use crate::workspace::path::canonical_workspace_root;

use crate::tools::markdown_links::core::{
    LinkIssue, MAX_MD_FILE_BYTES, RULE_ANCHOR, RULE_EXTERNAL, RULE_LOCAL, RULE_ROOT,
    build_http_client, collect_markdown_files, extract_link_hits, extract_ref_definitions,
    issue_json, issue_sarif,
};

use super::{
    MarkdownCheckParsed, MarkdownLinkScanCtx, TextReportSummary,
    markdown_check_process_one_link_hit, rel_path_for_report, render_text_report,
};

pub(super) fn markdown_check_links_inner(
    parsed: MarkdownCheckParsed,
    working_dir: &Path,
) -> String {
    let MarkdownCheckParsed {
        output_format,
        roots,
        max_files,
        max_depth,
        allowed_prefixes,
        ext_timeout,
        check_fragments,
    } = parsed;

    let ws_canonical = match canonical_workspace_root(working_dir) {
        Ok(p) => p,
        Err(e) => return format!("错误：{}", e),
    };

    let mut md_files: Vec<PathBuf> = Vec::new();
    let mut seen_files: HashSet<PathBuf> = HashSet::new();
    let mut root_errors: Vec<String> = Vec::new();
    for r in &roots {
        collect_markdown_files(
            working_dir,
            &ws_canonical,
            r,
            max_files,
            max_depth,
            &mut md_files,
            &mut seen_files,
            &mut root_errors,
        );
    }
    md_files.sort();

    let mut rel_ok = 0usize;
    let mut local_issues: Vec<LinkIssue> = Vec::new();
    let mut external_checked_ok = 0usize;
    let mut external_issues: Vec<LinkIssue> = Vec::new();
    let mut external_ignored = 0usize;
    let mut special_skipped = 0usize;
    let mut fragment_ok = 0usize;
    let mut fragment_bad = 0usize;
    let mut external_probe_requests = 0usize;
    let mut external_cache_hits = 0usize;
    let mut external_cache: HashMap<String, Result<u16, String>> = HashMap::new();
    let mut anchor_cache: HashMap<PathBuf, Result<HashSet<String>, String>> = HashMap::new();

    let timeout = Duration::from_secs(ext_timeout);
    let http_client = if allowed_prefixes.is_empty() {
        None
    } else {
        match build_http_client(timeout) {
            Ok(c) => Some(c),
            Err(e) => return format!("错误：{}", e),
        }
    };
    for e in root_errors {
        local_issues.push(LinkIssue {
            rule_id: RULE_ROOT,
            file: None,
            line: None,
            target: String::new(),
            message: e,
        });
    }

    for md_abs in &md_files {
        let meta = match std::fs::metadata(md_abs) {
            Ok(m) => m,
            Err(e) => {
                local_issues.push(LinkIssue {
                    rule_id: RULE_LOCAL,
                    file: Some(rel_path_for_report(&ws_canonical, md_abs)),
                    line: None,
                    target: "(file)".to_string(),
                    message: format!("无法读取元数据: {}", e),
                });
                continue;
            }
        };
        if meta.len() > MAX_MD_FILE_BYTES {
            local_issues.push(LinkIssue {
                rule_id: RULE_LOCAL,
                file: Some(rel_path_for_report(&ws_canonical, md_abs)),
                line: None,
                target: "(file)".to_string(),
                message: format!("文件超过 {} 字节，已跳过解析", MAX_MD_FILE_BYTES),
            });
            continue;
        }
        let content = match std::fs::read_to_string(md_abs) {
            Ok(s) => s,
            Err(e) => {
                local_issues.push(LinkIssue {
                    rule_id: RULE_LOCAL,
                    file: Some(rel_path_for_report(&ws_canonical, md_abs)),
                    line: None,
                    target: "(file)".to_string(),
                    message: format!("读取失败: {}", e),
                });
                continue;
            }
        };
        let ref_map = extract_ref_definitions(&content);
        let md_dir = md_abs.parent().unwrap_or(md_abs.as_path());
        let hits = extract_link_hits(&content, &ref_map);
        let md_rel = rel_path_for_report(&ws_canonical, md_abs);

        let mut scan_ctx = MarkdownLinkScanCtx {
            ws_canonical: &ws_canonical,
            allowed_prefixes: &allowed_prefixes,
            http_client: &http_client,
            check_fragments,
            rel_ok: &mut rel_ok,
            local_issues: &mut local_issues,
            external_checked_ok: &mut external_checked_ok,
            external_issues: &mut external_issues,
            external_ignored: &mut external_ignored,
            special_skipped: &mut special_skipped,
            fragment_ok: &mut fragment_ok,
            fragment_bad: &mut fragment_bad,
            external_probe_requests: &mut external_probe_requests,
            external_cache_hits: &mut external_cache_hits,
            external_cache: &mut external_cache,
            anchor_cache: &mut anchor_cache,
        };

        for h in hits {
            if let Err(e) = markdown_check_process_one_link_hit(
                &mut scan_ctx,
                h,
                md_abs,
                md_rel.as_str(),
                md_dir,
            ) {
                return e;
            }
        }
    }
    let text = render_text_report(
        TextReportSummary {
            ws_canonical: &ws_canonical,
            roots: &roots,
            md_files_len: md_files.len(),
            allowed_prefixes: &allowed_prefixes,
            rel_ok,
            fragment_ok,
            fragment_bad,
            external_checked_ok,
            external_ignored,
            special_skipped,
            external_probe_requests,
            external_cache_hits,
        },
        &local_issues,
        &external_issues,
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
                "markdown_files_scanned": md_files.len(),
                "relative_ok": rel_ok,
                "local_issues": local_issues.len(),
                "fragment_ok": fragment_ok,
                "fragment_issues": fragment_bad,
                "external_checked_ok": external_checked_ok,
                "external_issues": external_issues.len(),
                "external_ignored": external_ignored,
                "special_skipped": special_skipped,
                "external_probe_requests": external_probe_requests,
                "external_cache_hits": external_cache_hits,
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
