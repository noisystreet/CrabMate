//! 逐文件扫描 Markdown 链接。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use reqwest::blocking::Client;

use crate::tools::markdown_links::core::{
    LinkIssue, MAX_MD_FILE_BYTES, extract_link_hits, extract_ref_definitions,
};

use super::links_format::MarkdownCheckScanStats;
use super::{
    MarkdownCheckParsed, MarkdownLinkScanCtx, markdown_check_process_one_link_hit,
    rel_path_for_report,
};

pub(super) struct MarkdownCheckScanState {
    pub local_issues: Vec<LinkIssue>,
    pub external_issues: Vec<LinkIssue>,
    pub stats: MarkdownCheckScanStats,
    pub external_cache: HashMap<String, Result<u16, String>>,
    pub anchor_cache: HashMap<PathBuf, Result<HashSet<String>, String>>,
}

impl MarkdownCheckScanState {
    pub(super) fn new() -> Self {
        Self {
            local_issues: Vec::new(),
            external_issues: Vec::new(),
            stats: MarkdownCheckScanStats {
                rel_ok: 0,
                fragment_ok: 0,
                fragment_bad: 0,
                external_checked_ok: 0,
                external_ignored: 0,
                special_skipped: 0,
                external_probe_requests: 0,
                external_cache_hits: 0,
            },
            external_cache: HashMap::new(),
            anchor_cache: HashMap::new(),
        }
    }
}

pub(super) fn markdown_check_scan_all_files(
    parsed: &MarkdownCheckParsed,
    ws_canonical: &Path,
    md_files: &[PathBuf],
    http_client: &Option<Client>,
    state: &mut MarkdownCheckScanState,
) -> Option<String> {
    for md_abs in md_files {
        if let Some(err) =
            markdown_check_scan_one_md_file(parsed, ws_canonical, md_abs, http_client, state)
        {
            return Some(err);
        }
    }
    None
}

fn markdown_check_scan_one_md_file(
    parsed: &MarkdownCheckParsed,
    ws_canonical: &Path,
    md_abs: &Path,
    http_client: &Option<Client>,
    state: &mut MarkdownCheckScanState,
) -> Option<String> {
    let meta = match std::fs::metadata(md_abs) {
        Ok(m) => m,
        Err(e) => {
            state.local_issues.push(file_issue(
                ws_canonical,
                md_abs,
                format!("无法读取元数据: {}", e),
            ));
            return None;
        }
    };
    if meta.len() > MAX_MD_FILE_BYTES {
        state.local_issues.push(file_issue(
            ws_canonical,
            md_abs,
            format!("文件超过 {} 字节，已跳过解析", MAX_MD_FILE_BYTES),
        ));
        return None;
    }
    let content = match std::fs::read_to_string(md_abs) {
        Ok(s) => s,
        Err(e) => {
            state
                .local_issues
                .push(file_issue(ws_canonical, md_abs, format!("读取失败: {}", e)));
            return None;
        }
    };
    let ref_map = extract_ref_definitions(&content);
    let md_dir = md_abs.parent().unwrap_or(md_abs);
    let hits = extract_link_hits(&content, &ref_map);
    let md_rel = rel_path_for_report(ws_canonical, md_abs);

    let mut scan_ctx = MarkdownLinkScanCtx {
        ws_canonical,
        allowed_prefixes: &parsed.allowed_prefixes,
        http_client,
        check_fragments: parsed.check_fragments,
        rel_ok: &mut state.stats.rel_ok,
        local_issues: &mut state.local_issues,
        external_checked_ok: &mut state.stats.external_checked_ok,
        external_issues: &mut state.external_issues,
        external_ignored: &mut state.stats.external_ignored,
        special_skipped: &mut state.stats.special_skipped,
        fragment_ok: &mut state.stats.fragment_ok,
        fragment_bad: &mut state.stats.fragment_bad,
        external_probe_requests: &mut state.stats.external_probe_requests,
        external_cache_hits: &mut state.stats.external_cache_hits,
        external_cache: &mut state.external_cache,
        anchor_cache: &mut state.anchor_cache,
    };

    for h in hits {
        if let Err(e) =
            markdown_check_process_one_link_hit(&mut scan_ctx, h, md_abs, md_rel.as_str(), md_dir)
        {
            return Some(e);
        }
    }
    None
}

fn file_issue(ws_canonical: &Path, md_abs: &Path, message: String) -> LinkIssue {
    LinkIssue {
        rule_id: crate::tools::markdown_links::core::RULE_LOCAL,
        file: Some(rel_path_for_report(ws_canonical, md_abs)),
        line: None,
        target: "(file)".to_string(),
        message,
    }
}
