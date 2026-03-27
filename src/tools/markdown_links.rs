//! Markdown 内链接检查：校验工作区内相对目标是否存在；外链仅在配置允许前缀时发起 HEAD 探测。
//!
//! 路径与 `file` 工具一致：扫描根须为工作区相对路径，禁止 `..` 与绝对路径；解析目标时做词法归一化并限制在工作区根之下。

use crate::path_workspace::canonical_workspace_root;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use regex::Regex;
use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use std::sync::LazyLock;

/// 单个 Markdown 文件最多读取字节数（避免撑爆内存）
const MAX_MD_FILE_BYTES: u64 = 2 * 1024 * 1024;
const DEFAULT_MAX_FILES: usize = 300;
const ABS_MAX_FILES: usize = 3000;
const DEFAULT_MAX_DEPTH: usize = 24;
const ABS_MAX_DEPTH: usize = 80;
const DEFAULT_EXTERNAL_TIMEOUT_SECS: u64 = 10;
const ABS_MAX_EXTERNAL_TIMEOUT_SECS: u64 = 60;

static RE_INLINE: LazyLock<Regex> = LazyLock::new(|| {
    // 普通链接 [t](u) 与图片 ![a](u)
    Regex::new(r#"(?:!?\[([^\]]*)\])\(([^\s)]+)(?:\s+"[^"]*")?\)"#).expect("inline md link")
});
static RE_AUTOLINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<(https?://[^>\s]+)>").expect("autolink"));
static RE_REF_DEF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}\[([^\]]+)\]:[ \t]*<?([^ \t>]+)>?").expect("ref def"));
static RE_REF_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\[([^\]]*)\]").expect("ref link"));
const RULE_LOCAL: &str = "md-link-local";
const RULE_ANCHOR: &str = "md-link-anchor";
const RULE_EXTERNAL: &str = "md-link-external";
const RULE_ROOT: &str = "md-link-root";

#[derive(Debug, Clone)]
struct LinkHit {
    line: usize,
    raw: String,
}

#[derive(Debug, Clone)]
struct ParsedLocalTarget {
    path: String,
    had_fragment: bool,
    fragment_slug: Option<String>,
}

#[derive(Debug, Clone)]
struct LinkIssue {
    rule_id: &'static str,
    file: Option<String>,
    line: Option<usize>,
    target: String,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Text,
    Json,
    Sarif,
}

fn lexical_resolve_under(base: &Path, rel: &str) -> PathBuf {
    let rel_path = Path::new(rel);
    let mut out = base.to_path_buf();
    for c in rel_path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(x) => out.push(x),
            Component::Prefix(_) | Component::RootDir => {}
        }
    }
    out
}

fn strip_link_wrappers(raw: &str) -> String {
    let s = raw.trim();
    s.strip_prefix('<')
        .and_then(|x| x.strip_suffix('>'))
        .unwrap_or(s)
        .trim()
        .to_string()
}

fn ref_key(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn classify_scheme(url: &str) -> &'static str {
    let lower = url.to_lowercase();
    if lower.starts_with("mailto:") || lower.starts_with("tel:") {
        return "special";
    }
    if lower.starts_with("javascript:") || lower.starts_with("data:") {
        return "special";
    }
    "other"
}

fn is_external(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("//")
}

fn external_url_for_check(url: &str) -> String {
    if url.starts_with("//") {
        format!("https:{}", url)
    } else {
        url.to_string()
    }
}

/// 工具输出中避免暴露 URL 查询串（可能含 token）
fn url_for_display(url: &str) -> String {
    let u = external_url_for_check(url);
    match u.split_once('?') {
        Some((base, _)) => format!("{}?…", base),
        None => u,
    }
}

fn allowed_external(url: &str, prefixes: &[String]) -> bool {
    if prefixes.is_empty() {
        return false;
    }
    let full = external_url_for_check(url);
    prefixes.iter().any(|p| full.starts_with(p))
}

fn build_http_client(timeout: Duration) -> Result<Client, String> {
    Client::builder()
        .timeout(timeout)
        .redirect(Policy::limited(8))
        .build()
        .map_err(|e| format!("HTTP 客户端构建失败: {}", e))
}

fn head_check_url(client: &Client, url: &str) -> Result<u16, String> {
    let full = external_url_for_check(url);
    let resp = client
        .head(&full)
        .send()
        .map_err(|e| format!("请求失败: {}", e))?;
    let status = resp.status().as_u16();
    if status == 405 || status == 501 {
        let resp2 = client
            .get(&full)
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send()
            .map_err(|e| format!("GET 回退失败: {}", e))?;
        return Ok(resp2.status().as_u16());
    }
    Ok(status)
}

fn parse_output_format(raw: Option<&str>) -> Result<OutputFormat, String> {
    let raw = raw.map(str::trim).unwrap_or("text").to_ascii_lowercase();
    match raw.as_str() {
        "text" => Ok(OutputFormat::Text),
        "json" => Ok(OutputFormat::Json),
        "sarif" => Ok(OutputFormat::Sarif),
        _ => Err("错误：output_format 仅支持 text/json/sarif".to_string()),
    }
}

#[allow(clippy::too_many_arguments)] // 递归收集入口：根路径、上限与错误收集一次传入
fn collect_markdown_files(
    ws: &Path,
    ws_canonical: &Path,
    root_rel: &str,
    max_files: usize,
    max_depth: usize,
    out: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    errors: &mut Vec<String>,
) {
    if out.len() >= max_files {
        return;
    }
    let root_rel = root_rel.trim();
    if root_rel.is_empty() {
        errors.push("roots 中存在空路径".to_string());
        return;
    }
    if Path::new(root_rel).is_absolute() || root_rel.contains("..") {
        errors.push(format!("非法根路径（须为相对路径且无 ..）：{}", root_rel));
        return;
    }
    let abs = ws.join(root_rel);
    let abs = match abs.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            errors.push(format!("扫描根不存在或无法访问：{}", root_rel));
            return;
        }
    };
    if !abs.starts_with(ws_canonical) {
        errors.push(format!("扫描根越界：{}", root_rel));
        return;
    }

    fn walk(
        ws_canonical: &Path,
        dir: &Path,
        depth: usize,
        max_depth: usize,
        max_files: usize,
        out: &mut Vec<PathBuf>,
        seen: &mut HashSet<PathBuf>,
    ) {
        if out.len() >= max_files || depth > max_depth {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if out.len() >= max_files {
                break;
            }
            let p = e.path();
            let Ok(meta) = e.metadata() else {
                continue;
            };
            if meta.is_dir() {
                walk(ws_canonical, &p, depth + 1, max_depth, max_files, out, seen);
            } else if meta.is_file()
                && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("md"))
                && let Ok(can) = p.canonicalize()
                && can.starts_with(ws_canonical)
                && seen.insert(can.clone())
            {
                out.push(can);
            }
        }
    }

    let meta = match std::fs::metadata(&abs) {
        Ok(m) => m,
        Err(e) => {
            errors.push(format!("{}: {}", root_rel, e));
            return;
        }
    };
    if meta.is_file() {
        if abs
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("md"))
            && seen.insert(abs.clone())
        {
            out.push(abs);
        } else if !abs
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("md"))
        {
            errors.push(format!("扫描根是文件但不是 .md：{}", root_rel));
        }
    } else if meta.is_dir() {
        walk(ws_canonical, &abs, 0, max_depth, max_files, out, seen);
    }
}

fn extract_ref_definitions(content: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for line in content.lines() {
        if line.len() > 16_384 {
            continue;
        }
        if let Some(c) = RE_REF_DEF.captures(line) {
            let id = c.get(1).map(|x| x.as_str()).unwrap_or("");
            let url = c.get(2).map(|x| x.as_str()).unwrap_or("");
            let key = ref_key(id);
            let cleaned = strip_link_wrappers(url);
            if !key.is_empty() && !cleaned.is_empty() {
                m.insert(key, cleaned);
            }
        }
    }
    m
}

fn extract_link_hits(content: &str, ref_map: &HashMap<String, String>) -> Vec<LinkHit> {
    let mut hits = Vec::new();
    for (line_no, line) in content.lines().enumerate() {
        let line_no = line_no + 1;
        if line.len() > 64 * 1024 {
            continue;
        }
        for c in RE_INLINE.captures_iter(line) {
            let raw = c.get(2).map(|x| x.as_str()).unwrap_or("");
            let u = strip_link_wrappers(raw);
            if !u.is_empty() {
                hits.push(LinkHit {
                    line: line_no,
                    raw: u,
                });
            }
        }
        for c in RE_AUTOLINK.captures_iter(line) {
            let raw = c.get(1).map(|x| x.as_str()).unwrap_or("");
            let u = strip_link_wrappers(raw);
            if !u.is_empty() {
                hits.push(LinkHit {
                    line: line_no,
                    raw: u,
                });
            }
        }
        for c in RE_REF_LINK.captures_iter(line) {
            let text = c.get(1).map(|x| x.as_str()).unwrap_or("");
            let id_part = c.get(2).map(|x| x.as_str()).unwrap_or("");
            let key = if id_part.is_empty() {
                ref_key(text)
            } else {
                ref_key(id_part)
            };
            if let Some(url) = ref_map.get(&key)
                && !url.is_empty()
            {
                hits.push(LinkHit {
                    line: line_no,
                    raw: url.clone(),
                });
            }
        }
    }
    hits
}

fn rel_path_for_report(ws_canonical: &Path, abs: &Path) -> String {
    abs.strip_prefix(ws_canonical)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| abs.to_string_lossy().to_string())
}

fn from_hex(v: u8) -> Option<u8> {
    match v {
        b'0'..=b'9' => Some(v - b'0'),
        b'a'..=b'f' => Some(v - b'a' + 10),
        b'A'..=b'F' => Some(v - b'A' + 10),
        _ => None,
    }
}

fn percent_decode_lossy(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2]))
        {
            out.push((h << 4) | l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn slugify_heading_anchor(input: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in input.chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
            last_dash = false;
        } else if ch.is_whitespace() && !out.is_empty() && !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn normalize_fragment_slug(raw_fragment: &str) -> Option<String> {
    let decoded = percent_decode_lossy(raw_fragment.trim());
    let slug = slugify_heading_anchor(&decoded);
    if slug.is_empty() { None } else { Some(slug) }
}

fn parse_local_target(url: &str) -> ParsedLocalTarget {
    let cleaned = strip_link_wrappers(url);
    let (path_raw, frag_raw, had_fragment) = match cleaned.split_once('#') {
        Some((path, frag)) => (path, frag, true),
        None => (cleaned.as_str(), "", false),
    };
    let path = path_raw.split('?').next().unwrap_or("").trim().to_string();
    let fragment_slug = if had_fragment {
        normalize_fragment_slug(frag_raw)
    } else {
        None
    };
    ParsedLocalTarget {
        path,
        had_fragment,
        fragment_slug,
    }
}

fn parse_heading_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let mut text = trimmed[level..].trim_start();
    if text.is_empty() {
        return None;
    }
    text = text.trim_end();
    while let Some(stripped) = text.strip_suffix('#') {
        text = stripped.trim_end();
    }
    if text.is_empty() { None } else { Some(text) }
}

fn extract_markdown_anchor_set(content: &str) -> HashSet<String> {
    let mut anchors = HashSet::new();
    let mut dup_count: HashMap<String, usize> = HashMap::new();
    for line in content.lines() {
        let Some(title) = parse_heading_text(line) else {
            continue;
        };
        let base = slugify_heading_anchor(title);
        if base.is_empty() {
            continue;
        }
        let c = dup_count.entry(base.clone()).or_insert(0);
        let actual = if *c == 0 {
            base.clone()
        } else {
            format!("{}-{}", base, *c)
        };
        *c += 1;
        anchors.insert(actual);
    }
    anchors
}

fn load_markdown_anchor_set(path: &Path) -> Result<HashSet<String>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("读取元数据失败: {}", e))?;
    if meta.len() > MAX_MD_FILE_BYTES {
        return Err(format!("文件超过 {} 字节，无法校验锚点", MAX_MD_FILE_BYTES));
    }
    let content = std::fs::read_to_string(path).map_err(|e| format!("读取失败: {}", e))?;
    Ok(extract_markdown_anchor_set(&content))
}

fn issue_text(issue: &LinkIssue) -> String {
    match (&issue.file, issue.line) {
        (Some(file), Some(line)) => {
            format!("{}:{} -> {}（{}）", file, line, issue.target, issue.message)
        }
        (Some(file), None) => format!("{} -> {}（{}）", file, issue.target, issue.message),
        (None, _) => issue.message.clone(),
    }
}

fn issue_json(issue: &LinkIssue) -> serde_json::Value {
    serde_json::json!({
        "rule_id": issue.rule_id,
        "file": issue.file,
        "line": issue.line,
        "target": issue.target,
        "message": issue.message,
    })
}

fn issue_sarif(issue: &LinkIssue) -> serde_json::Value {
    let mut result = serde_json::Map::new();
    result.insert(
        "ruleId".to_string(),
        serde_json::Value::String(issue.rule_id.to_string()),
    );
    result.insert(
        "message".to_string(),
        serde_json::json!({ "text": issue_text(issue) }),
    );
    if let Some(file) = issue.file.as_ref() {
        let loc = if let Some(line) = issue.line {
            serde_json::json!({
                "physicalLocation": {
                    "artifactLocation": { "uri": file },
                    "region": { "startLine": line }
                }
            })
        } else {
            serde_json::json!({
                "physicalLocation": {
                    "artifactLocation": { "uri": file }
                }
            })
        };
        result.insert("locations".to_string(), serde_json::json!([loc]));
    }
    serde_json::Value::Object(result)
}

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

/// 参数 JSON：
/// - `roots`?: string[]，默认 `["README.md","docs"]`
/// - `max_files`?: number，默认 300，上限 3000
/// - `max_depth`?: number，目录递归深度，默认 24，上限 80
/// - `allowed_external_prefixes`?: string[]，非空时：以这些前缀开头的 http(s)/协议相对 URL 会发 HEAD（失败时 GET Range 回退）
/// - `external_timeout_secs`?: number，默认 10，上限 60
/// - `check_fragments`?: bool，默认 true；是否校验 `#fragment`（按目标 Markdown 标题锚点）
/// - `output_format`?: string，默认 text；可选 text/json/sarif
pub fn markdown_check_links(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let output_format = match parse_output_format(v.get("output_format").and_then(|x| x.as_str())) {
        Ok(fmt) => fmt,
        Err(e) => return e,
    };

    let roots: Vec<String> = v
        .get("roots")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .filter(|roots_vec: &Vec<String>| !roots_vec.is_empty())
        .unwrap_or_else(|| vec!["README.md".into(), "docs".into()]);

    let max_files = v
        .get("max_files")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_MAX_FILES)
        .clamp(1, ABS_MAX_FILES);

    let max_depth = v
        .get("max_depth")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_MAX_DEPTH)
        .clamp(1, ABS_MAX_DEPTH);

    let allowed_prefixes: Vec<String> = v
        .get("allowed_external_prefixes")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let ext_timeout = v
        .get("external_timeout_secs")
        .and_then(|n| n.as_u64())
        .unwrap_or(DEFAULT_EXTERNAL_TIMEOUT_SECS)
        .clamp(1, ABS_MAX_EXTERNAL_TIMEOUT_SECS);
    let check_fragments = v
        .get("check_fragments")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);

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

        for h in hits {
            let url = h.raw.trim();
            if url.is_empty() {
                continue;
            }
            if classify_scheme(url) == "special" {
                special_skipped += 1;
                continue;
            }
            if is_external(url) {
                if allowed_external(url, &allowed_prefixes) {
                    let full = external_url_for_check(url);
                    let checked = if let Some(cached) = external_cache.get(&full) {
                        external_cache_hits += 1;
                        cached.clone()
                    } else {
                        external_probe_requests += 1;
                        let Some(client) = http_client.as_ref() else {
                            return "错误：HTTP 客户端未初始化".to_string();
                        };
                        let result = head_check_url(client, url);
                        external_cache.insert(full, result.clone());
                        result
                    };
                    match checked {
                        Ok(code) if (200..400).contains(&code) => {
                            external_checked_ok += 1;
                        }
                        Ok(code) => {
                            external_issues.push(LinkIssue {
                                rule_id: RULE_EXTERNAL,
                                file: Some(md_rel.clone()),
                                line: Some(h.line),
                                target: url_for_display(url),
                                message: format!("HTTP {}", code),
                            });
                        }
                        Err(e) => {
                            external_issues.push(LinkIssue {
                                rule_id: RULE_EXTERNAL,
                                file: Some(md_rel.clone()),
                                line: Some(h.line),
                                target: url_for_display(url),
                                message: e,
                            });
                        }
                    }
                } else {
                    external_ignored += 1;
                }
                continue;
            }
            let target = parse_local_target(url);
            if target.path.is_empty() && !target.had_fragment {
                continue;
            }
            if Path::new(&target.path).is_absolute() {
                local_issues.push(LinkIssue {
                    rule_id: RULE_LOCAL,
                    file: Some(md_rel.clone()),
                    line: Some(h.line),
                    target: strip_link_wrappers(url),
                    message: "非相对路径，已标为问题；请改为相对链接".to_string(),
                });
                continue;
            }
            let target_abs = if target.path.is_empty() {
                md_abs.clone()
            } else {
                lexical_resolve_under(md_dir, &target.path)
            };
            if !target_abs.starts_with(&ws_canonical) {
                local_issues.push(LinkIssue {
                    rule_id: RULE_LOCAL,
                    file: Some(md_rel.clone()),
                    line: Some(h.line),
                    target: strip_link_wrappers(url),
                    message: "解析后越出工作区".to_string(),
                });
                continue;
            }
            if target_abs.exists() {
                rel_ok += 1;
            } else {
                local_issues.push(LinkIssue {
                    rule_id: RULE_LOCAL,
                    file: Some(md_rel.clone()),
                    line: Some(h.line),
                    target: strip_link_wrappers(url),
                    message: "目标不存在".to_string(),
                });
                continue;
            }
            if !check_fragments || !target.had_fragment {
                continue;
            }
            let Some(fragment_slug) = target.fragment_slug.as_ref() else {
                fragment_bad += 1;
                local_issues.push(LinkIssue {
                    rule_id: RULE_ANCHOR,
                    file: Some(md_rel.clone()),
                    line: Some(h.line),
                    target: strip_link_wrappers(url),
                    message: "锚点为空或无法解析".to_string(),
                });
                continue;
            };
            if !target_abs
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("md"))
            {
                fragment_bad += 1;
                local_issues.push(LinkIssue {
                    rule_id: RULE_ANCHOR,
                    file: Some(md_rel.clone()),
                    line: Some(h.line),
                    target: strip_link_wrappers(url),
                    message: "锚点校验仅支持 Markdown 目标".to_string(),
                });
                continue;
            }
            let anchors = if let Some(cached) = anchor_cache.get(&target_abs) {
                cached.clone()
            } else {
                let loaded = load_markdown_anchor_set(&target_abs);
                anchor_cache.insert(target_abs.clone(), loaded.clone());
                loaded
            };
            match anchors {
                Ok(set) => {
                    if set.contains(fragment_slug) {
                        fragment_ok += 1;
                    } else {
                        fragment_bad += 1;
                        local_issues.push(LinkIssue {
                            rule_id: RULE_ANCHOR,
                            file: Some(md_rel.clone()),
                            line: Some(h.line),
                            target: strip_link_wrappers(url),
                            message: format!("锚点不存在: #{}", fragment_slug),
                        });
                    }
                }
                Err(e) => {
                    fragment_bad += 1;
                    local_issues.push(LinkIssue {
                        rule_id: RULE_ANCHOR,
                        file: Some(md_rel.clone()),
                        line: Some(h.line),
                        target: strip_link_wrappers(url),
                        message: format!("锚点校验失败: {}", e),
                    });
                }
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
        OutputFormat::Text => text,
        OutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
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
        OutputFormat::Sarif => {
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

#[cfg(test)]
mod tests;
