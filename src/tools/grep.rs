//! 在工作区内按正则/关键词搜索文件内容。
//!
//! 功能：给定 pattern（Rust 正则语法），从工作区根目录（run_command_working_dir）开始递归搜索文件，
//! 返回匹配文件路径 + 行号 + 行文本片段。支持可选的子路径、大小写控制和结果数量限制。

use regex::RegexBuilder;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// 搜索参数：由 JSON 中的字段解析而来
struct SearchParams {
    pattern: String,
    sub_path: Option<String>,
    max_results: usize,
    case_insensitive: bool,
    ignore_hidden: bool,
}

const DEFAULT_MAX_RESULTS: usize = 200;
const MAX_FILE_SIZE_BYTES: usize = 2 * 1024 * 1024; // 单个文件最大搜索大小：2MB

pub fn run(args_json: &str, workspace_root: &Path) -> String {
    let params = match parse_params(args_json) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // 解析 pattern 为正则
    let re = match RegexBuilder::new(&params.pattern)
        .case_insensitive(params.case_insensitive)
        .build()
    {
        Ok(r) => r,
        Err(e) => return format!("错误：无效的正则表达式：{}", e),
    };

    // 解析搜索起点路径（相对 workspace 根）
    let root = match resolve_search_root(workspace_root, params.sub_path.as_deref()) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let mut results = Vec::new();
    let mut visited = 0usize;

    if let Err(e) = walk_and_search(
        &root,
        &re,
        &mut results,
        &mut visited,
        params.max_results,
        params.ignore_hidden,
    ) {
        return format!("搜索过程中发生错误：{}", e);
    }

    if results.is_empty() {
        return format!(
            "未找到匹配：\"{}\"（共遍历 {} 个文件，搜索根目录：{}）",
            params.pattern,
            visited,
            root.display()
        );
    }

    let mut out = String::new();
    out.push_str(&format!(
        "搜索模式：\"{}\"，根目录：{}\n匹配结果（最多 {} 条，实际 {} 条）：\n\n",
        params.pattern,
        root.display(),
        params.max_results,
        results.len()
    ));
    for (path, line_no, line) in results {
        out.push_str(&format!("{}:{}: {}\n", path.display(), line_no, line));
    }
    out
}

fn parse_params(args_json: &str) -> Result<SearchParams, String> {
    let v: serde_json::Value = serde_json::from_str(args_json)
        .map_err(|e| format!("参数 JSON 无效：{}", e))?;
    let pattern = v
        .get("pattern")
        .and_then(|p| p.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "缺少 pattern 参数".to_string())?;
    let sub_path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let max_results = v
        .get("max_results")
        .and_then(|m| m.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(DEFAULT_MAX_RESULTS);
    let case_insensitive = v
        .get("case_insensitive")
        .and_then(|b| b.as_bool())
        .unwrap_or(true);
    let ignore_hidden = v
        .get("ignore_hidden")
        .and_then(|b| b.as_bool())
        .unwrap_or(true);
    Ok(SearchParams {
        pattern,
        sub_path,
        max_results,
        case_insensitive,
        ignore_hidden,
    })
}

fn resolve_search_root(base: &Path, sub: Option<&str>) -> Result<PathBuf, String> {
    match sub {
        None => Ok(base.to_path_buf()),
        Some(s) => {
            let sub_path = Path::new(s);
            if sub_path.is_absolute() {
                return Err("路径必须为相对于工作区的相对路径，不能使用绝对路径".to_string());
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

fn walk_and_search(
    root: &Path,
    re: &regex::Regex,
    results: &mut Vec<(PathBuf, usize, String)>,
    visited_files: &mut usize,
    max_results: usize,
    ignore_hidden: bool,
) -> io::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    if root.is_file() {
        search_in_file(root, re, results, visited_files, max_results)?;
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if ignore_hidden && name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk_and_search(&path, re, results, visited_files, max_results, ignore_hidden)?;
            if results.len() >= max_results {
                break;
            }
        } else if path.is_file() {
            search_in_file(&path, re, results, visited_files, max_results)?;
            if results.len() >= max_results {
                break;
            }
        }
    }
    Ok(())
}

fn search_in_file(
    path: &Path,
    re: &regex::Regex,
    results: &mut Vec<(PathBuf, usize, String)>,
    visited_files: &mut usize,
    max_results: usize,
) -> io::Result<()> {
    *visited_files += 1;
    let mut f = fs::File::open(path)?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;
    if buf.len() > MAX_FILE_SIZE_BYTES {
        // 对超大文件只读取前 MAX_FILE_SIZE_BYTES 字节，以避免占用过多内存
        buf.truncate(MAX_FILE_SIZE_BYTES);
    }
    for (idx, line) in buf.lines().enumerate() {
        if re.is_match(line) {
            results.push((path.to_path_buf(), idx + 1, line.to_string()));
            if results.len() >= max_results {
                break;
            }
        }
    }
    Ok(())
}

