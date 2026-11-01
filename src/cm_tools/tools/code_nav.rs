//! 代码理解与导航：符号引用搜索、Rust 单文件结构大纲。

#[path = "code_nav_outline.rs"]
mod code_nav_outline;

use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::symbol;
use crate::cm_tools::tools::tool_param_types::FindReferencesArgs;
use code_nav_outline::collect_outline;
use regex::Regex;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::code_nav_paths::{resolve_search_root, resolve_workspace_file, search_io_failed};
use super::text_truncate::truncate_line;

const MAX_FILE_SIZE_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_MAX_RESULTS: usize = 80;
const MAX_RESULTS_LIMIT: usize = 300;
const DEFAULT_MAX_OUTLINE_ITEMS: usize = 200;

/// `find_references`：在工作区内搜索某标识符的「引用」（词边界匹配，默认跳过疑似定义行）。
///
/// 迁 `Typed` 后 `walk_rs` 的 IO 故障是显式 `code_nav_search_failed`：旧实现把它写成
/// `搜索过程中发生错误：…`，该措辞不命中 `looks_like_failure` 的任何关键词，
/// 于是「没读到文件」被当成「没有引用」。
#[allow(clippy::result_large_err)]
pub fn find_references_try(args_json: &str, workspace_root: &Path) -> Result<String, ToolError> {
    let args: FindReferencesArgs =
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

    // 与 JSON schema 一致：case_sensitive 默认 false 表示忽略大小写
    let case_insensitive = !args.case_sensitive;

    let exclude_definitions = args.exclude_definitions;

    let include_hidden = args.include_hidden;

    let root = resolve_search_root(workspace_root, sub_path.as_deref())?;

    let esc = regex::escape(&symbol);
    let ref_pat = format!(r"\b{}\b", esc);
    let ref_re = regex::RegexBuilder::new(&ref_pat)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|e| {
            // 规则由 `regex::escape` 后的符号名拼出，编译失败即内部不变量破坏。
            ToolError::internal_code(
                "code_nav_regex_failed",
                format!("错误：无法构建引用搜索规则：{e}"),
            )
        })?;

    let mut results: Vec<(PathBuf, usize, String)> = Vec::new();
    let mut visited = 0usize;
    let mut walk_ctx = RsWalkCtx {
        ref_re: &ref_re,
        symbol: &symbol,
        exclude_definitions,
        case_insensitive,
        results: &mut results,
        visited_files: &mut visited,
        max_results,
        ignore_hidden_dirs: include_hidden,
    };
    walk_rs(&root, &mut walk_ctx)?;

    if results.is_empty() {
        return Ok(format!(
            "未找到引用：\"{}\"（遍历 {} 个 .rs 文件，根：{}）",
            symbol,
            visited,
            root.display()
        ));
    }

    let mut out = String::new();
    out.push_str(&format!(
        "引用搜索：\"{}\"，根目录：{}\n（词边界匹配；exclude_definitions={}）\n匹配 {} 条（上限 {}）：\n\n",
        symbol,
        root.display(),
        exclude_definitions,
        results.len(),
        max_results
    ));
    for (path, line_no, line) in results {
        out.push_str(&format!(
            "{}:{}: {}\n",
            path.display(),
            line_no,
            truncate_line(&line, 320)
        ));
    }
    Ok(out.trim_end().to_string())
}

/// 列出单个 Rust 源文件中的模块级结构（fn/mod/struct/enum/trait/impl 等行摘要）。
///
/// 三条失败分支各有码：`path` 不是文件 / 不是 `.rs` 属 `InvalidInput`（换参数可修），
/// 读盘失败属 `External`。旧实现把它们写成 `错误：…` / `读取文件失败：…`，
/// 前者靠前缀偶然判对、后者只因为正文恰好含「失败」二字。
#[allow(clippy::result_large_err)]
pub fn rust_file_outline_try(
    args_json: &str,
    workspace_root: &Path,
) -> Result<String, ToolError> {
    let v = crate::cm_tools::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(rel) = path else {
        return Err(ToolError::invalid_args(
            "错误：缺少 path 参数".to_string(),
        ));
    };

    let include_use = v
        .get("include_use")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    let max_items = v
        .get("max_items")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(DEFAULT_MAX_OUTLINE_ITEMS)
        .min(500);

    let target = resolve_workspace_file(workspace_root, rel)?;
    if !target.is_file() {
        return Err(ToolError::with_code(
            "code_nav_path_not_file",
            "错误：path 必须是已存在的文件".to_string(),
        ));
    }
    if target.extension().and_then(|e| e.to_str()) != Some("rs") {
        return Err(ToolError::with_code(
            "code_nav_path_not_rust_file",
            "错误：当前仅支持 .rs 文件大纲".to_string(),
        ));
    }

    let mut buf = String::new();
    fs::File::open(&target)
        .and_then(|mut f| f.read_to_string(&mut buf))
        .map_err(|e| {
            ToolError::external_code("code_nav_read_failed", format!("错误：读取文件失败：{e}"))
        })?;
    if buf.len() > MAX_FILE_SIZE_BYTES {
        buf = super::output_util::truncate_to_char_boundary(&buf, MAX_FILE_SIZE_BYTES);
    }

    let items = collect_outline(&buf, include_use, max_items);
    if items.is_empty() {
        return Ok(format!(
            "文件 {} 中未匹配到常见顶层结构（可尝试 include_use=true）",
            rel
        ));
    }

    let mut out = String::new();
    out.push_str(&format!(
        "Rust 文件大纲：{}（{} 项，最多 {}）\n\n",
        rel,
        items.len(),
        max_items
    ));
    for (line_no, summary) in items {
        out.push_str(&format!("{:>5}: {}\n", line_no, summary));
    }
    Ok(out.trim_end().to_string())
}

struct RsWalkCtx<'a> {
    ref_re: &'a Regex,
    symbol: &'a str,
    exclude_definitions: bool,
    case_insensitive: bool,
    results: &'a mut Vec<(PathBuf, usize, String)>,
    visited_files: &'a mut usize,
    max_results: usize,
    ignore_hidden_dirs: bool,
}

#[allow(clippy::result_large_err)] // `ToolError` 含正文解析快照（`parsed`），与 runner 契约一致
fn walk_rs(root: &Path, ctx: &mut RsWalkCtx<'_>) -> Result<(), ToolError> {
    if !root.exists() {
        return Ok(());
    }
    if root.is_file() {
        return walk_rs_one_file(root, ctx);
    }
    walk_rs_dir(root, ctx)
}

/// 目录递归主体：隐藏目录/非 `.rs` 文件由 [`is_rs_entry_visitable`] 过滤，命中上限即停止。
#[allow(clippy::result_large_err)] // `ToolError` 含正文解析快照（`parsed`），与 runner 契约一致
fn walk_rs_dir(dir: &Path, ctx: &mut RsWalkCtx<'_>) -> Result<(), ToolError> {
    for entry in fs::read_dir(dir).map_err(|e| search_io_failed(&e.to_string()))? {
        let entry = entry.map_err(|e| search_io_failed(&e.to_string()))?;
        let path = entry.path();
        let name = entry.file_name();
        if !is_rs_entry_visitable(&path, &name, ctx) {
            continue;
        }
        if path.is_dir() {
            walk_rs(&path, ctx)?;
        } else {
            walk_rs_one_file(&path, ctx)?;
        }
        if ctx.results.len() >= ctx.max_results {
            break;
        }
    }
    Ok(())
}

/// 是否进入该条目：目录始终进入；文件仅当后缀为 `.rs`；隐藏目录按 `ignore_hidden_dirs` 跳过。
fn is_rs_entry_visitable(path: &Path, name: &std::ffi::OsStr, ctx: &RsWalkCtx<'_>) -> bool {
    if ctx.ignore_hidden_dirs && name.to_string_lossy().starts_with('.') {
        return false;
    }
    if path.is_dir() {
        return true;
    }
    path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("rs")
}

#[allow(clippy::result_large_err)] // `ToolError` 含正文解析快照（`parsed`），与 runner 契约一致
fn walk_rs_one_file(path: &Path, ctx: &mut RsWalkCtx<'_>) -> Result<(), ToolError> {
    if path.extension().and_then(|e| e.to_str()) != Some("rs") {
        return Ok(());
    }
    *ctx.visited_files += 1;
    search_refs_in_file(
        path,
        ctx.ref_re,
        ctx.symbol,
        ctx.exclude_definitions,
        ctx.case_insensitive,
        ctx.results,
        ctx.max_results,
    )
}

#[allow(clippy::result_large_err)] // `ToolError` 含正文解析快照（`parsed`），与 runner 契约一致
fn search_refs_in_file(
    path: &Path,
    ref_re: &Regex,
    symbol: &str,
    exclude_definitions: bool,
    case_insensitive: bool,
    results: &mut Vec<(PathBuf, usize, String)>,
    max_results: usize,
) -> Result<(), ToolError> {
    let mut buf = String::new();
    let mut f = fs::File::open(path).map_err(|e| search_io_failed(&e.to_string()))?;
    f.read_to_string(&mut buf)
        .map_err(|e| search_io_failed(&e.to_string()))?;
    if buf.len() > MAX_FILE_SIZE_BYTES {
        buf = super::output_util::truncate_to_char_boundary(&buf, MAX_FILE_SIZE_BYTES);
    }
    for (idx, line) in buf.lines().enumerate() {
        if !ref_re.is_match(line) {
            continue;
        }
        if exclude_definitions
            && symbol::line_looks_like_rust_definition(line, symbol, case_insensitive)
        {
            continue;
        }
        results.push((path.to_path_buf(), idx + 1, line.to_string()));
        if results.len() >= max_results {
            break;
        }
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_finds_fn_and_mod() {
        let src = r"
mod foo {
}
pub fn bar(x: u32) -> u32 { x }
";
        let v = collect_outline(src, false, 50);
        assert!(v.iter().any(|(_, s)| s.contains("mod foo")));
        assert!(v.iter().any(|(_, s)| s.contains("fn bar")));
    }

    #[test]
    fn outline_respects_max_items() {
        let mut s = String::new();
        for i in 0..30 {
            s.push_str(&format!("fn f{i}() {{}}\n"));
        }
        let v = collect_outline(&s, false, 5);
        assert_eq!(v.len(), 5);
    }
}
