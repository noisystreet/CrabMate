//! 在工作区内定位某个“符号”的潜在定义位置（Rust 侧）。
//!
//! 目标是给模型提供一个可用的“代码定位”工具：给出 symbol 名称后，返回匹配行及上下文。

use regex::RegexBuilder;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::tool_param_types::FindSymbolArgs;

use super::code_nav_paths::{resolve_search_root, search_io_failed};

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

/// `find_symbol`：在工作区内定位符号定义行。
///
/// 迁 `Typed` 时顺带清掉一处**静默成功**：`resolve_root` 的路径拒绝（绝对路径 / `..` /
/// 越界）旧实现直接 `return e`，正文以「路径」开头、不含「失败 / 超时」，
/// `looks_like_failure` 因此判 `ok: true` —— 模型会把「路径被拒」读成
/// 「这个符号在工作区里没有定义」。现在分别是三个 `PolicyDenied` 码。
#[allow(clippy::result_large_err)]
pub fn run_try(args_json: &str, workspace_root: &Path) -> Result<String, ToolError> {
    let params = parse_params(args_json)?;

    let root = resolve_search_root(workspace_root, params.sub_path.as_deref())?;

    let kind = params.kind.as_deref().map(|s| s.trim().to_lowercase());
    let re = build_symbol_regex(&params.symbol, kind.as_deref(), params.case_insensitive).map_err(
        |e| {
            // 规则由本模块用 `regex::escape` 后的符号名拼出，编译失败即内部不变量破坏。
            ToolError::internal_code(
                "code_nav_regex_failed",
                format!("错误：无效的符号搜索规则：{e}"),
            )
        },
    )?;

    let mut results: Vec<(PathBuf, usize, String)> = Vec::new();
    let mut visited = 0usize;

    walk_and_match(
        &root,
        &re,
        &mut results,
        &mut visited,
        params.max_results,
        params.include_hidden,
    )?;

    if results.is_empty() {
        return Ok(format!(
            "未找到符号：\"{}\"（遍历 {} 个文件，搜索根目录：{}）",
            params.symbol,
            visited,
            root.display()
        ));
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
            && let Ok(ctx) = read_context_lines(&path, line_no, params.context_lines)
        {
            out.push_str(&ctx);
        }
        out.push('\n');
    }

    Ok(out.trim_end().to_string())
}

#[allow(clippy::result_large_err)] // `ToolError` 含 legacy 解析快照，与 runner 契约一致
fn parse_params(args_json: &str) -> Result<SymbolParams, ToolError> {
    let args: FindSymbolArgs =
        crate::cm_tools::tools::parse_args_typed(args_json).map_err(ToolError::invalid_args)?;

    let symbol = args.symbol.trim().to_string();
    if symbol.is_empty() {
        return Err(ToolError::invalid_args(
            "错误：缺少 symbol 参数".to_string(),
        ));
    }

    let sub_path = args
        .path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let max_results = (args
        .max_results
        .unwrap_or(DEFAULT_MAX_RESULTS as u64)
        .max(1) as usize)
        .min(MAX_RESULTS_LIMIT);

    let case_insensitive = args.case_insensitive;

    let include_hidden = args.include_hidden;

    let context_lines = args
        .context_lines
        .unwrap_or(DEFAULT_CONTEXT_LINES as u64)
        .min(1_000_000) as usize;

    let kind = args
        .kind
        .as_deref()
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

/// 判断单行是否像 Rust 中某符号的「定义」行（与 `find_symbol` 规则一致），供 `find_references` 排除定义处重复命中。
pub fn line_looks_like_rust_definition(line: &str, symbol: &str, case_insensitive: bool) -> bool {
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

#[allow(clippy::result_large_err)] // `ToolError` 含 legacy 解析快照，与 runner 契约一致
fn walk_and_match(
    root: &Path,
    re: &regex::Regex,
    results: &mut Vec<(PathBuf, usize, String)>,
    visited_files: &mut usize,
    max_results: usize,
    ignore_hidden: bool,
) -> Result<(), ToolError> {
    if !root.exists() {
        return Ok(());
    }
    if root.is_file() {
        *visited_files += 1;
        search_file(root, re, results, max_results)?;
        return Ok(());
    }
    walk_and_match_dir(root, re, results, visited_files, max_results, ignore_hidden)
}

#[allow(clippy::result_large_err)] // `ToolError` 含 legacy 解析快照，与 runner 契约一致
fn walk_and_match_dir(
    root: &Path,
    re: &regex::Regex,
    results: &mut Vec<(PathBuf, usize, String)>,
    visited_files: &mut usize,
    max_results: usize,
    ignore_hidden: bool,
) -> Result<(), ToolError> {
    for entry in fs::read_dir(root).map_err(|e| search_io_failed(&e.to_string()))? {
        let entry = entry.map_err(|e| search_io_failed(&e.to_string()))?;
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
        } else if path.is_file()
            && visit_walk_match_file(&path, re, results, visited_files, max_results)
        {
            break;
        }
    }
    Ok(())
}

/// 扫描单个文件：计数访问次数；命中且结果已满时返回 `true`（调用方据此 `break`）。
fn visit_walk_match_file(
    path: &Path,
    re: &regex::Regex,
    results: &mut Vec<(PathBuf, usize, String)>,
    visited_files: &mut usize,
    max_results: usize,
) -> bool {
    *visited_files += 1;
    search_file(path, re, results, max_results).unwrap_or(false) && results.len() >= max_results
}

#[allow(clippy::result_large_err)] // `ToolError` 含 legacy 解析快照，与 runner 契约一致
fn search_file(
    path: &Path,
    re: &regex::Regex,
    results: &mut Vec<(PathBuf, usize, String)>,
    max_results: usize,
) -> Result<bool, ToolError> {
    let mut buf = String::new();
    let mut f = fs::File::open(path).map_err(|e| search_io_failed(&e.to_string()))?;
    f.read_to_string(&mut buf)
        .map_err(|e| search_io_failed(&e.to_string()))?;
    if buf.len() > MAX_FILE_SIZE_BYTES {
        buf = super::output_util::truncate_to_char_boundary(&buf, MAX_FILE_SIZE_BYTES);
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
