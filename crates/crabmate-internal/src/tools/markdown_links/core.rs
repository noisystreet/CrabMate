use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use regex::Regex;
use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use std::sync::LazyLock;

/// 单个 Markdown 文件最多读取字节数（避免撑爆内存）
pub const MAX_MD_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const DEFAULT_MAX_FILES: usize = 300;
pub const ABS_MAX_FILES: usize = 3000;
pub const DEFAULT_MAX_DEPTH: usize = 24;
pub const ABS_MAX_DEPTH: usize = 80;
pub const DEFAULT_EXTERNAL_TIMEOUT_SECS: u64 = 10;
pub const ABS_MAX_EXTERNAL_TIMEOUT_SECS: u64 = 60;

pub static RE_INLINE: LazyLock<Regex> = LazyLock::new(|| {
    // 普通链接 [t](u) 与图片 ![a](u)
    Regex::new(r#"(?:!?\[([^\]]*)\])\(([^\s)]+)(?:\s+"[^"]*")?\)"#).expect("inline md link")
});
pub static RE_AUTOLINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<(https?://[^>\s]+)>").expect("autolink"));
pub static RE_REF_DEF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}\[([^\]]+)\]:[ \t]*<?([^ \t>]+)>?").expect("ref def"));
pub static RE_REF_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\[([^\]]*)\]").expect("ref link"));
pub const RULE_LOCAL: &str = "md-link-local";
pub const RULE_ANCHOR: &str = "md-link-anchor";
pub const RULE_EXTERNAL: &str = "md-link-external";
pub const RULE_ROOT: &str = "md-link-root";

#[derive(Debug, Clone)]
pub struct LinkHit {
    pub line: usize,
    pub raw: String,
}

#[derive(Debug, Clone)]
pub struct ParsedLocalTarget {
    pub path: String,
    pub had_fragment: bool,
    pub fragment_slug: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LinkIssue {
    pub rule_id: &'static str,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub target: String,
    pub message: String,
}

pub fn lexical_resolve_under(base: &Path, rel: &str) -> PathBuf {
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

pub fn strip_link_wrappers(raw: &str) -> String {
    let s = raw.trim();
    s.strip_prefix('<')
        .and_then(|x| x.strip_suffix('>'))
        .unwrap_or(s)
        .trim()
        .to_string()
}

pub fn ref_key(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn classify_scheme(url: &str) -> &'static str {
    let lower = url.to_lowercase();
    if lower.starts_with("mailto:") || lower.starts_with("tel:") {
        return "special";
    }
    if lower.starts_with("javascript:") || lower.starts_with("data:") {
        return "special";
    }
    "other"
}

pub fn is_external(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("//")
}

pub fn external_url_for_check(url: &str) -> String {
    if url.starts_with("//") {
        format!("https:{}", url)
    } else {
        url.to_string()
    }
}

/// 工具输出中避免暴露 URL 查询串（可能含 token）
pub fn url_for_display(url: &str) -> String {
    let u = external_url_for_check(url);
    match u.split_once('?') {
        Some((base, _)) => format!("{}?…", base),
        None => u,
    }
}

pub fn allowed_external(url: &str, prefixes: &[String]) -> bool {
    if prefixes.is_empty() {
        return false;
    }
    let full = external_url_for_check(url);
    prefixes.iter().any(|p| full.starts_with(p))
}

pub fn build_http_client(timeout: Duration) -> Result<Client, String> {
    Client::builder()
        .timeout(timeout)
        .redirect(Policy::limited(8))
        .build()
        .map_err(|e| format!("HTTP 客户端构建失败: {}", e))
}

pub fn head_check_url(client: &Client, url: &str) -> Result<u16, String> {
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

#[allow(clippy::too_many_arguments)] // 递归收集入口：根路径、上限与错误收集一次传入
pub fn collect_markdown_files(
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

pub fn extract_ref_definitions(content: &str) -> HashMap<String, String> {
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

pub fn extract_link_hits(content: &str, ref_map: &HashMap<String, String>) -> Vec<LinkHit> {
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

pub fn rel_path_for_report(ws_canonical: &Path, abs: &Path) -> String {
    abs.strip_prefix(ws_canonical)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| abs.to_string_lossy().to_string())
}

pub fn from_hex(v: u8) -> Option<u8> {
    match v {
        b'0'..=b'9' => Some(v - b'0'),
        b'a'..=b'f' => Some(v - b'a' + 10),
        b'A'..=b'F' => Some(v - b'A' + 10),
        _ => None,
    }
}

pub fn percent_decode_lossy(input: &str) -> String {
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

pub fn slugify_heading_anchor(input: &str) -> String {
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

pub fn normalize_fragment_slug(raw_fragment: &str) -> Option<String> {
    let decoded = percent_decode_lossy(raw_fragment.trim());
    let slug = slugify_heading_anchor(&decoded);
    if slug.is_empty() { None } else { Some(slug) }
}

pub fn parse_local_target(url: &str) -> ParsedLocalTarget {
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

pub fn parse_heading_text(line: &str) -> Option<&str> {
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

pub fn extract_markdown_anchor_set(content: &str) -> HashSet<String> {
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

pub fn load_markdown_anchor_set(path: &Path) -> Result<HashSet<String>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("读取元数据失败: {}", e))?;
    if meta.len() > MAX_MD_FILE_BYTES {
        return Err(format!("文件超过 {} 字节，无法校验锚点", MAX_MD_FILE_BYTES));
    }
    let content = std::fs::read_to_string(path).map_err(|e| format!("读取失败: {}", e))?;
    Ok(extract_markdown_anchor_set(&content))
}

pub fn issue_text(issue: &LinkIssue) -> String {
    match (&issue.file, issue.line) {
        (Some(file), Some(line)) => {
            format!("{}:{} -> {}（{}）", file, line, issue.target, issue.message)
        }
        (Some(file), None) => format!("{} -> {}（{}）", file, issue.target, issue.message),
        (None, _) => issue.message.clone(),
    }
}

pub fn issue_json(issue: &LinkIssue) -> serde_json::Value {
    serde_json::json!({
        "rule_id": issue.rule_id,
        "file": issue.file,
        "line": issue.line,
        "target": issue.target,
        "message": issue.message,
    })
}

pub fn issue_sarif(issue: &LinkIssue) -> serde_json::Value {
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
