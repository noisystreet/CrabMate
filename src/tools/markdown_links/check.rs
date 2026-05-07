use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::tools::tool_param_types::MarkdownCheckLinksOutputFormat;
use crate::workspace::path::canonical_workspace_root;
use reqwest::blocking::Client;

use super::core::*;

struct TextReportSummary<'a> {
    ws_canonical: &'a Path,
    roots: &'a [String],
    md_files_len: usize,
    allowed_prefixes: &'a [String],
    rel_ok: usize,
    fragment_ok: usize,
    fragment_bad: usize,
    external_checked_ok: usize,
    external_ignored: usize,
    special_skipped: usize,
    external_probe_requests: usize,
    external_cache_hits: usize,
}

fn render_text_report(
    summary: TextReportSummary<'_>,
    local_issues: &[LinkIssue],
    external_issues: &[LinkIssue],
) -> String {
    let TextReportSummary {
        ws_canonical,
        roots,
        md_files_len,
        allowed_prefixes,
        rel_ok,
        fragment_ok,
        fragment_bad,
        external_checked_ok,
        external_ignored,
        special_skipped,
        external_probe_requests,
        external_cache_hits,
    } = summary;
    let mut out = String::new();
    out.push_str("Markdown 链接检查\n");
    out.push_str(&format!("工作区: {}\n", ws_canonical.display()));
    out.push_str(&format!("扫描根: {}\n", roots.join(", ")));
    out.push_str(&format!("已扫描 .md 文件: {} 个\n", md_files_len));
    out.push_str(&format!(
        "统计: 相对链接存在 {} / 本地问题 {} / 锚点通过 {} / 锚点问题 {} / 外链(允许列表内)成功 {} / 外链失败 {} / 外链未校验 {} / 特殊协议跳过 {}\n",
        rel_ok,
        local_issues.len(),
        fragment_ok,
        fragment_bad,
        external_checked_ok,
        external_issues.len(),
        external_ignored,
        special_skipped
    ));
    if allowed_prefixes.is_empty() {
        out.push_str("说明: 未配置 allowed_external_prefixes，所有 http(s)/协议相对链接均只做计数、不发网络请求。\n");
    } else {
        out.push_str(&format!("外链允许前缀: {}\n", allowed_prefixes.join(" | ")));
        out.push_str(&format!(
            "外链探测请求（去重后）: {}，缓存命中: {}\n",
            external_probe_requests, external_cache_hits
        ));
    }
    out.push('\n');
    if !local_issues.is_empty() {
        out.push_str("【本地路径/锚点问题】\n");
        for issue in local_issues {
            out.push_str(&issue_text(issue));
            out.push('\n');
        }
        out.push('\n');
    }
    if !external_issues.is_empty() {
        out.push_str("【外链探测失败】\n");
        for issue in external_issues {
            out.push_str(&issue_text(issue));
            out.push('\n');
        }
        out.push('\n');
    }
    let problems = local_issues.len() + external_issues.len();
    if problems == 0 {
        out.push_str("结论: 未发现已检查的失效链接。\n");
    } else {
        out.push_str(&format!(
            "结论: 发现 {} 处问题，请根据上文路径修复。\n",
            problems
        ));
    }
    out.trim_end().to_string()
}

struct MarkdownCheckParsed {
    output_format: MarkdownCheckLinksOutputFormat,
    roots: Vec<String>,
    max_files: usize,
    max_depth: usize,
    allowed_prefixes: Vec<String>,
    ext_timeout: u64,
    check_fragments: bool,
}

fn parse_markdown_check_args(
    args: &crate::tools::tool_param_types::MarkdownCheckLinksArgs,
) -> Result<MarkdownCheckParsed, String> {
    let output_format = args.output_format.unwrap_or_default();

    let roots: Vec<String> = args
        .roots
        .as_ref()
        .map(|arr| {
            arr.iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|roots_vec: &Vec<String>| !roots_vec.is_empty())
        .unwrap_or_else(|| vec!["README.md".into(), "docs".into()]);

    let max_files = args
        .max_files
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_MAX_FILES)
        .clamp(1, ABS_MAX_FILES);

    let max_depth = args
        .max_depth
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_MAX_DEPTH)
        .clamp(1, ABS_MAX_DEPTH);

    let allowed_prefixes: Vec<String> = args
        .allowed_external_prefixes
        .as_ref()
        .map(|arr| {
            arr.iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let ext_timeout = args
        .external_timeout_secs
        .map(|n| n as u64)
        .unwrap_or(DEFAULT_EXTERNAL_TIMEOUT_SECS)
        .clamp(1, ABS_MAX_EXTERNAL_TIMEOUT_SECS);

    Ok(MarkdownCheckParsed {
        output_format,
        roots,
        max_files,
        max_depth,
        allowed_prefixes,
        ext_timeout,
        check_fragments: args.check_fragments,
    })
}

struct MarkdownLinkScanCtx<'a> {
    ws_canonical: &'a Path,
    allowed_prefixes: &'a [String],
    http_client: &'a Option<Client>,
    check_fragments: bool,
    rel_ok: &'a mut usize,
    local_issues: &'a mut Vec<LinkIssue>,
    external_checked_ok: &'a mut usize,
    external_issues: &'a mut Vec<LinkIssue>,
    external_ignored: &'a mut usize,
    special_skipped: &'a mut usize,
    fragment_ok: &'a mut usize,
    fragment_bad: &'a mut usize,
    external_probe_requests: &'a mut usize,
    external_cache_hits: &'a mut usize,
    external_cache: &'a mut HashMap<String, Result<u16, String>>,
    anchor_cache: &'a mut HashMap<PathBuf, Result<HashSet<String>, String>>,
}

/// 处理单个 Markdown 文件内的一条链接命中；`Err` 为致命错误（需提前结束整个检查并返回该字符串）。
fn markdown_check_process_one_link_hit(
    ctx: &mut MarkdownLinkScanCtx<'_>,
    h: LinkHit,
    md_abs: &Path,
    md_rel: &str,
    md_dir: &Path,
) -> Result<(), String> {
    let url = h.raw.trim();
    if url.is_empty() {
        return Ok(());
    }
    if classify_scheme(url) == "special" {
        *ctx.special_skipped += 1;
        return Ok(());
    }
    if is_external(url) {
        if allowed_external(url, ctx.allowed_prefixes) {
            let full = external_url_for_check(url);
            let checked = if let Some(cached) = ctx.external_cache.get(&full) {
                *ctx.external_cache_hits += 1;
                cached.clone()
            } else {
                *ctx.external_probe_requests += 1;
                let Some(client) = ctx.http_client.as_ref() else {
                    return Err("错误：HTTP 客户端未初始化".to_string());
                };
                let result = head_check_url(client, url);
                ctx.external_cache.insert(full, result.clone());
                result
            };
            match checked {
                Ok(code) if (200..400).contains(&code) => {
                    *ctx.external_checked_ok += 1;
                }
                Ok(code) => {
                    ctx.external_issues.push(LinkIssue {
                        rule_id: RULE_EXTERNAL,
                        file: Some(md_rel.to_string()),
                        line: Some(h.line),
                        target: url_for_display(url),
                        message: format!("HTTP {}", code),
                    });
                }
                Err(e) => {
                    ctx.external_issues.push(LinkIssue {
                        rule_id: RULE_EXTERNAL,
                        file: Some(md_rel.to_string()),
                        line: Some(h.line),
                        target: url_for_display(url),
                        message: e,
                    });
                }
            }
        } else {
            *ctx.external_ignored += 1;
        }
        return Ok(());
    }
    let target = parse_local_target(url);
    if target.path.is_empty() && !target.had_fragment {
        return Ok(());
    }
    if Path::new(&target.path).is_absolute() {
        ctx.local_issues.push(LinkIssue {
            rule_id: RULE_LOCAL,
            file: Some(md_rel.to_string()),
            line: Some(h.line),
            target: strip_link_wrappers(url),
            message: "非相对路径，已标为问题；请改为相对链接".to_string(),
        });
        return Ok(());
    }
    let target_abs = if target.path.is_empty() {
        md_abs.to_path_buf()
    } else {
        lexical_resolve_under(md_dir, &target.path)
    };
    if !target_abs.starts_with(ctx.ws_canonical) {
        ctx.local_issues.push(LinkIssue {
            rule_id: RULE_LOCAL,
            file: Some(md_rel.to_string()),
            line: Some(h.line),
            target: strip_link_wrappers(url),
            message: "解析后越出工作区".to_string(),
        });
        return Ok(());
    }
    if target_abs.exists() {
        *ctx.rel_ok += 1;
    } else {
        ctx.local_issues.push(LinkIssue {
            rule_id: RULE_LOCAL,
            file: Some(md_rel.to_string()),
            line: Some(h.line),
            target: strip_link_wrappers(url),
            message: "目标不存在".to_string(),
        });
        return Ok(());
    }
    if !ctx.check_fragments || !target.had_fragment {
        return Ok(());
    }
    let Some(fragment_slug) = target.fragment_slug.as_ref() else {
        *ctx.fragment_bad += 1;
        ctx.local_issues.push(LinkIssue {
            rule_id: RULE_ANCHOR,
            file: Some(md_rel.to_string()),
            line: Some(h.line),
            target: strip_link_wrappers(url),
            message: "锚点为空或无法解析".to_string(),
        });
        return Ok(());
    };
    if !target_abs
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case("md"))
    {
        *ctx.fragment_bad += 1;
        ctx.local_issues.push(LinkIssue {
            rule_id: RULE_ANCHOR,
            file: Some(md_rel.to_string()),
            line: Some(h.line),
            target: strip_link_wrappers(url),
            message: "锚点校验仅支持 Markdown 目标".to_string(),
        });
        return Ok(());
    }
    let anchors = if let Some(cached) = ctx.anchor_cache.get(&target_abs) {
        cached.clone()
    } else {
        let loaded = load_markdown_anchor_set(&target_abs);
        ctx.anchor_cache.insert(target_abs.clone(), loaded.clone());
        loaded
    };
    match anchors {
        Ok(set) => {
            if set.contains(fragment_slug) {
                *ctx.fragment_ok += 1;
            } else {
                *ctx.fragment_bad += 1;
                ctx.local_issues.push(LinkIssue {
                    rule_id: RULE_ANCHOR,
                    file: Some(md_rel.to_string()),
                    line: Some(h.line),
                    target: strip_link_wrappers(url),
                    message: format!("锚点不存在: #{}", fragment_slug),
                });
            }
        }
        Err(e) => {
            *ctx.fragment_bad += 1;
            ctx.local_issues.push(LinkIssue {
                rule_id: RULE_ANCHOR,
                file: Some(md_rel.to_string()),
                line: Some(h.line),
                target: strip_link_wrappers(url),
                message: format!("锚点校验失败: {}", e),
            });
        }
    }
    Ok(())
}

fn markdown_check_links_inner(parsed: MarkdownCheckParsed, working_dir: &Path) -> String {
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

/// 参数 JSON：
/// - `roots`?: string[]，默认 `["README.md","docs"]`
/// - `max_files`?: number，默认 300，上限 3000
/// - `max_depth`?: number，目录递归深度，默认 24，上限 80
/// - `allowed_external_prefixes`?: string[]，非空时：以这些前缀开头的 http(s)/协议相对 URL 会发 HEAD（失败时 GET Range 回退）
/// - `external_timeout_secs`?: number，默认 10，上限 60
/// - `check_fragments`?: bool，默认 true；是否校验 `#fragment`（按目标 Markdown 标题锚点）
/// - `output_format`?: string，默认 text；可选 text/json/sarif
pub fn markdown_check_links(args_json: &str, working_dir: &Path) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: crate::tools::tool_param_types::MarkdownCheckLinksArgs =
        match serde_json::from_value(v) {
            Ok(a) => a,
            Err(e) => return format!("参数 JSON 与 markdown_check_links 形状不一致: {e}"),
        };
    match parse_markdown_check_args(&args) {
        Ok(parsed) => markdown_check_links_inner(parsed, working_dir),
        Err(e) => e,
    }
}
