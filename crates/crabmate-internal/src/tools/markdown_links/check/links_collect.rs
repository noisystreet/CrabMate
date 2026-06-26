//! 收集 Markdown 扫描目标与工作区上下文。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use reqwest::blocking::Client;

use crate::tools::markdown_links::core::{LinkIssue, RULE_ROOT, collect_markdown_files};
use crate::workspace::path::canonical_workspace_root;

use super::MarkdownCheckParsed;

pub(super) struct MarkdownCheckCollected {
    pub ws_canonical: PathBuf,
    pub md_files: Vec<PathBuf>,
    pub http_client: Option<Client>,
    pub root_issues: Vec<LinkIssue>,
}

pub(super) fn markdown_check_collect_files(
    parsed: &MarkdownCheckParsed,
    working_dir: &Path,
) -> Result<MarkdownCheckCollected, String> {
    let ws_canonical = canonical_workspace_root(working_dir).map_err(|e| e.to_string())?;

    let mut md_files: Vec<PathBuf> = Vec::new();
    let mut seen_files: HashSet<PathBuf> = HashSet::new();
    let mut root_errors: Vec<String> = Vec::new();
    for r in &parsed.roots {
        collect_markdown_files(
            working_dir,
            &ws_canonical,
            r,
            parsed.max_files,
            parsed.max_depth,
            &mut md_files,
            &mut seen_files,
            &mut root_errors,
        );
    }
    md_files.sort();

    let http_client = if parsed.allowed_prefixes.is_empty() {
        None
    } else {
        let timeout = std::time::Duration::from_secs(parsed.ext_timeout);
        Some(
            crate::tools::markdown_links::core::build_http_client(timeout)
                .map_err(|e| e.to_string())?,
        )
    };

    let root_issues = root_errors
        .into_iter()
        .map(|e| LinkIssue {
            rule_id: RULE_ROOT,
            file: None,
            line: None,
            target: String::new(),
            message: e,
        })
        .collect();

    Ok(MarkdownCheckCollected {
        ws_canonical,
        md_files,
        http_client,
        root_issues,
    })
}
