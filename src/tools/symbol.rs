//! 在工作区内定位某个“符号”的潜在定义位置（Rust 侧）。
//!
//! 目标是给模型提供一个可用的“代码定位”工具：给出 symbol 名称后，返回匹配行及上下文。

use regex::RegexBuilder;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_FILE_SIZE_BYTES: usize = 2 * 1024 * 1024; // 2MB
const DEFAULT_MAX_RESULTS: usize = 30;
const DEFAULT_CONTEXT_LINES: usize = 2;
const MAX_RESULTS_LIMIT: usize = 200;

struct SymbolParams {
    symbol: String,
    sub_path: Option<String>,
    max_results: usize,
    case_insensitive: bool,
    include_hidden: bool,
    context_lines: usize,
    kind: Option<String>,
}

pub fn run(args_json: &str, workspace_root: &Path) -> String {
    let params = match parse_params(args_json) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let root = match resolve_root(workspace_root, params.sub_path.as_deref()) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let kind = params.kind.as_deref().map(|s| s.trim().to_lowercase());
    let re = match build_symbol_regex(&params.symbol, kind.as_deref(), params.case_insensitive) {
        Ok(r) => r,
        Err(e) => return format!("错误：无效的符号搜索规则：{}", e),
    };

    let mut results: Vec<(PathBuf, usize, String)> = Vec::new();
    let mut visited = 0usize;

    let res = walk_and_match(
        &root,
        &re,
        &mut results,
        &mut visited,
        params.max_results,
        params.include_hidden,
    );
    if let Err(e) = res {
        return format!("搜索过程中发生错误：{}", e);
    }

    if results.is_empty() {
        return format!(
            "未找到符号：\"{}\"（遍历 {} 个文件，搜索根目录：{}）",
            params.symbol,
            visited,
            root.display()
        );
    }

    let mut out = String::new();
    out.push_str(&format!(
        "符号搜索：\"{}\"，根目录：{}\n匹配结果（最多 {} 条，实际 {} 条）：\n\n",
        params.symbol,
        root.display(),
        params.max_results,
        results.len()
    ));

    for (path, line_no, line) in results {
        out.push_str(&format!(
            "{}:{}: {}\n",
            path.display(),
            line_no,
            truncate_line(&line, 300)
        ));
        // 可选上下文：重新读取文件并输出附近行
        if params.context_lines > 0
            && let Ok(ctx) = read_context_lines(&path, line_no, params.context_lines) {
                out.push_str(&ctx);
            }
        out.push('\n');
    }

    out.trim_end().to_string()
}

fn parse_params(args_json: &str) -> Result<SymbolParams, String> {
    let v: serde_json::Value =
        serde_json::from_str(args_json).map_err(|e| format!("参数 JSON 无效：{}", e))?;

    let symbol = v
        .get("symbol")
        .and_then(|s| s.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| "缺少 symbol 参数".to_string())?;

    let sub_path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let max_results = v
        .get("max_results")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(DEFAULT_MAX_RESULTS)
        .min(MAX_RESULTS_LIMIT);

    let case_insensitive = v
        .get("case_insensitive")
        .and_then(|b| b.as_bool())
        .unwrap_or(true);

    let include_hidden = v
        .get("include_hidden")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    let context_lines = v
        .get("context_lines")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_CONTEXT_LINES);

    let kind = v
        .get("kind")
        .and_then(|k| k.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Ok(SymbolParams {
        symbol,
        sub_path,
        max_results,
        case_insensitive,
        include_hidden,
        context_lines,
        kind,
    })
}

fn resolve_root(base: &Path, sub: Option<&str>) -> Result<PathBuf, String> {
    match sub {
        None => Ok(base.to_path_buf()),
        Some(s) => {
            let sub_path = Path::new(s);
            if sub_path.is_absolute() {
                return Err("路径必须为工作区内相对路径，不能使用绝对路径".to_string());
            }
            if s.contains("..") {
                return Err("路径不能包含 ..".to_string());
            }
            let joined = base.join(sub_path);
            let canon_base = base
                .canonicalize()
                .map_err(|e| format!("工作区根目录无法解析: {}", e))?;
            let canon_joined = joined
                .canonicalize()
                .map_err(|e| format!("搜索路径无法解析: {}", e))?;
            if !canon_joined.starts_with(&canon_base) {
                return Err("搜索路径不能超出工作区根目录".to_string());
            }
            Ok(canon_joined)
        }
    }
}

/// 判断单行是否像 Rust 中某符号的「定义」行（与 `find_symbol` 规则一致），供 `find_references` 排除定义处重复命中。
pub(crate) fn line_looks_like_rust_definition(
    line: &str,
    symbol: &str,
    case_insensitive: bool,
) -> bool {
    match build_symbol_regex(symbol, None, case_insensitive) {
        Ok(re) => re.is_match(line),
        Err(_) => false,
    }
}

fn build_symbol_regex(
    symbol: &str,
    kind: Option<&str>,
    case_insensitive: bool,
) -> Result<regex::Regex, String> {
    // Rust 常见定义：fn/struct/enum/trait/const/static/type/mod
    let esc = regex::escape(symbol);
    let patterns: Vec<String> = match kind {
        Some("fn") => vec![format!(r"(?m)^\s*(pub\s+)?(async\s+)?fn\s+{}\b", esc)],
        Some("struct") => vec![format!(r"(?m)^\s*(pub\s+)?struct\s+{}\b", esc)],
        Some("enum") => vec![format!(r"(?m)^\s*(pub\s+)?enum\s+{}\b", esc)],
        Some("trait") => vec![format!(r"(?m)^\s*(pub\s+)?trait\s+{}\b", esc)],
        Some("const") => vec![format!(r"(?m)^\s*(pub\s+)?const\s+{}\b", esc)],
        Some("static") => vec![format!(r"(?m)^\s*(pub\s+)?static\s+{}\b", esc)],
        Some("type") => vec![format!(r"(?m)^\s*(pub\s+)?type\s+{}\b", esc)],
        Some("mod") => vec![format!(r"(?m)^\s*(pub\s+)?mod\s+{}\b", esc)],
        // any
        _ => vec![
            format!(r"(?m)^\s*(pub\s+)?(async\s+)?fn\s+{}\b", esc),
            format!(r"(?m)^\s*(pub\s+)?struct\s+{}\b", esc),
            format!(r"(?m)^\s*(pub\s+)?enum\s+{}\b", esc),
            format!(r"(?m)^\s*(pub\s+)?trait\s+{}\b", esc),
            format!(r"(?m)^\s*(pub\s+)?const\s+{}\b", esc),
            format!(r"(?m)^\s*(pub\s+)?static\s+{}\b", esc),
            format!(r"(?m)^\s*(pub\s+)?type\s+{}\b", esc),
            format!(r"(?m)^\s*(pub\s+)?mod\s+{}\b", esc),
        ],
    };

    let combined = patterns.join("|");
    RegexBuilder::new(&combined)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|e| e.to_string())
}

fn walk_and_match(
    root: &Path,
    re: &regex::Regex,
    results: &mut Vec<(PathBuf, usize, String)>,
    visited_files: &mut usize,
    max_results: usize,
    ignore_hidden: bool,
) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    if root.is_file() {
        *visited_files += 1;
        match search_file(root, re, results, max_results) {
            Ok(found) => {
                if found {
                    // already enforced by max_results check in search_file
                }
                return Ok(());
            }
            Err(e) => return Err(e),
        }
    }
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if ignore_hidden && name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk_and_match(
                &path,
                re,
                results,
                visited_files,
                max_results,
                ignore_hidden,
            )?;
        } else if path.is_file() {
            *visited_files += 1;
            if search_file(&path, re, results, max_results).unwrap_or(false)
                && results.len() >= max_results {
                    break;
                }
        }
    }
    Ok(())
}

fn search_file(
    path: &Path,
    re: &regex::Regex,
    results: &mut Vec<(PathBuf, usize, String)>,
    max_results: usize,
) -> Result<bool, String> {
    let mut buf = String::new();
    let mut f = fs::File::open(path).map_err(|e| e.to_string())?;
    f.read_to_string(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() > MAX_FILE_SIZE_BYTES {
        buf.truncate(MAX_FILE_SIZE_BYTES);
    }

    for (idx, line) in buf.lines().enumerate() {
        if re.is_match(line) {
            results.push((path.to_path_buf(), idx + 1, line.to_string()));
            if results.len() >= max_results {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn read_context_lines(path: &PathBuf, line_no: usize, ctx: usize) -> Result<String, String> {
    let s = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = s.lines().collect();
    if lines.is_empty() {
        return Ok(String::new());
    }
    let start = line_no.saturating_sub(ctx).max(1);
    let end = (line_no + ctx).min(lines.len());
    let mut out = String::new();
    out.push_str(&format!("  context {}-{}:\n", start, end));
    for i in start..=end {
        out.push_str(&format!("    {}|{}\n", i, lines[i - 1]));
    }
    Ok(out)
}

fn truncate_line(s: &str, max_chars: usize) -> String {
    let s = s.trim_end();
    let mut chars = s.chars();
    let mut out = String::new();
    for _ in 0..max_chars {
        if let Some(ch) = chars.next() {
            out.push(ch);
        } else {
            return out;
        }
    }
    if s.chars().count() > max_chars {
        format!("{}... (截断)", out)
    } else {
        out
    }
}

