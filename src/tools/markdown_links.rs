//! Markdown 内链接检查：校验工作区内相对目标是否存在；外链仅在配置允许前缀时发起 HEAD 探测。
//!
//! 路径与 `file` 工具一致：扫描根须为工作区相对路径，禁止 `..` 与绝对路径；解析目标时做词法归一化并限制在工作区根之下。

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use regex::Regex;
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

#[derive(Debug, Clone)]
struct LinkHit {
    line: usize,
    raw: String,
}

fn canonical_workspace_root(base: &Path) -> Result<PathBuf, String> {
    base.canonicalize()
        .map_err(|e| format!("工作目录无法解析: {}", e))
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

fn strip_link_url(raw: &str) -> String {
    let s = raw.trim();
    let s = s
        .strip_prefix('<')
        .and_then(|x| x.strip_suffix('>'))
        .unwrap_or(s);
    let s = s.split('#').next().unwrap_or("");
    s.split('?').next().unwrap_or("").trim().to_string()
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

fn head_check_url(url: &str, timeout: Duration) -> Result<u16, String> {
    let full = external_url_for_check(url);
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .redirect(Policy::limited(8))
        .build()
        .map_err(|e| format!("HTTP 客户端构建失败: {}", e))?;

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
            if !key.is_empty() && !url.is_empty() {
                m.insert(key, strip_link_url(url));
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
            let u = strip_link_url(raw);
            if !u.is_empty() && !u.starts_with('#') {
                hits.push(LinkHit {
                    line: line_no,
                    raw: u,
                });
            }
        }
        for c in RE_AUTOLINK.captures_iter(line) {
            let raw = c.get(1).map(|x| x.as_str()).unwrap_or("");
            let u = strip_link_url(raw);
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
                && !url.starts_with('#')
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

/// 参数 JSON：
/// - `roots`?: string[]，默认 `["README.md","docs"]`
/// - `max_files`?: number，默认 300，上限 3000
/// - `max_depth`?: number，目录递归深度，默认 24，上限 80
/// - `allowed_external_prefixes`?: string[]，非空时：以这些前缀开头的 http(s)/协议相对 URL 会发 HEAD（失败时 GET Range 回退）
/// - `external_timeout_secs`?: number，默认 10，上限 60
pub fn markdown_check_links(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
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
    let mut rel_broken: Vec<String> = Vec::new();
    let mut external_checked_ok = 0usize;
    let mut external_broken: Vec<String> = Vec::new();
    let mut external_ignored = 0usize;
    let mut special_skipped = 0usize;

    let timeout = Duration::from_secs(ext_timeout);

    for md_abs in &md_files {
        let meta = match std::fs::metadata(md_abs) {
            Ok(m) => m,
            Err(e) => {
                rel_broken.push(format!(
                    "{}: 无法读取元数据: {}",
                    rel_path_for_report(&ws_canonical, md_abs),
                    e
                ));
                continue;
            }
        };
        if meta.len() > MAX_MD_FILE_BYTES {
            rel_broken.push(format!(
                "{}: 文件超过 {} 字节，已跳过解析",
                rel_path_for_report(&ws_canonical, md_abs),
                MAX_MD_FILE_BYTES
            ));
            continue;
        }
        let content = match std::fs::read_to_string(md_abs) {
            Ok(s) => s,
            Err(e) => {
                rel_broken.push(format!(
                    "{}: 读取失败: {}",
                    rel_path_for_report(&ws_canonical, md_abs),
                    e
                ));
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
                    match head_check_url(url, timeout) {
                        Ok(code) if (200..400).contains(&code) => {
                            external_checked_ok += 1;
                        }
                        Ok(code) => {
                            external_broken.push(format!(
                                "{}:{} -> {}（HTTP {}）",
                                md_rel,
                                h.line,
                                url_for_display(url),
                                code
                            ));
                        }
                        Err(e) => {
                            external_broken.push(format!(
                                "{}:{} -> {}（{}）",
                                md_rel,
                                h.line,
                                url_for_display(url),
                                e
                            ));
                        }
                    }
                } else {
                    external_ignored += 1;
                }
                continue;
            }
            if Path::new(url).is_absolute() {
                rel_broken.push(format!(
                    "{}:{} -> {}（非相对路径，已标为问题；请改为相对链接）",
                    md_rel, h.line, url
                ));
                continue;
            }
            let target = lexical_resolve_under(md_dir, url);
            if !target.starts_with(&ws_canonical) {
                rel_broken.push(format!(
                    "{}:{} -> {}（解析后越出工作区）",
                    md_rel, h.line, url
                ));
                continue;
            }
            if target.exists() {
                rel_ok += 1;
            } else {
                rel_broken.push(format!("{}:{} -> {}（目标不存在）", md_rel, h.line, url));
            }
        }
    }

    let mut out = String::new();
    out.push_str("Markdown 链接检查\n");
    out.push_str(&format!("工作区: {}\n", ws_canonical.display()));
    out.push_str(&format!("扫描根: {}\n", roots.join(", ")));
    if !root_errors.is_empty() {
        out.push_str("根路径提示:\n");
        for e in &root_errors {
            out.push_str(&format!("  - {}\n", e));
        }
    }
    out.push_str(&format!("已扫描 .md 文件: {} 个\n", md_files.len()));
    out.push_str(&format!(
        "统计: 相对链接存在 {} / 相对问题 {} / 外链(允许列表内)成功 {} / 外链失败 {} / 外链未校验 {} / 特殊协议跳过 {}\n",
        rel_ok,
        rel_broken.len(),
        external_checked_ok,
        external_broken.len(),
        external_ignored,
        special_skipped
    ));
    if allowed_prefixes.is_empty() {
        out.push_str("说明: 未配置 allowed_external_prefixes，所有 http(s)/协议相对链接均只做计数、不发网络请求。\n");
    } else {
        out.push_str(&format!("外链允许前缀: {}\n", allowed_prefixes.join(" | ")));
    }
    out.push('\n');

    if !rel_broken.is_empty() {
        out.push_str("【相对路径或其它本地问题】\n");
        for line in &rel_broken {
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }
    if !external_broken.is_empty() {
        out.push_str("【外链探测失败】\n");
        for line in &external_broken {
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }

    let problems = rel_broken.len() + external_broken.len();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn lexical_and_broken_relative() {
        let tmp = std::env::temp_dir().join(format!("crabmate_md_links_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("docs")).unwrap();
        fs::write(
            tmp.join("README.md"),
            "# Hi\n[ok](./docs/a.md)\n[bad](./docs/nope.md)\n",
        )
        .unwrap();
        fs::write(tmp.join("docs/a.md"), "x").unwrap();

        let args = serde_json::json!({ "roots": ["README.md"] }).to_string();
        let out = markdown_check_links(&args, &tmp);
        assert!(
            out.contains("目标不存在"),
            "expected missing link report: {}",
            out
        );
        assert!(out.contains("docs/nope.md"), "{}", out);
    }

    #[test]
    fn ref_style_link_resolves() {
        let tmp = std::env::temp_dir().join(format!("crabmate_md_refs_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("d")).unwrap();
        fs::write(tmp.join("x.md"), "[t][r]\n[r]: d/f.md\n").unwrap();
        fs::write(tmp.join("d/f.md"), "ok").unwrap();
        let args = serde_json::json!({ "roots": ["x.md"] }).to_string();
        let out = markdown_check_links(&args, &tmp);
        assert!(
            out.contains("未发现已检查的失效链接"),
            "ref link should resolve: {}",
            out
        );
    }
}
