//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use glob::Pattern;
use std::path::Path;

use super::path::{
    canonical_workspace_root, path_for_tool_display, resolve_for_read,
    tool_user_error_from_workspace_path,
};

/// glob_files：默认/上限
const GLOB_DEFAULT_MAX_DEPTH: usize = 20;
const GLOB_ABS_MAX_DEPTH: usize = 100;
const GLOB_DEFAULT_MAX_RESULTS: usize = 200;
const GLOB_ABS_MAX_RESULTS: usize = 5000;

/// list_tree：默认/上限
const TREE_DEFAULT_MAX_DEPTH: usize = 8;
const TREE_ABS_MAX_DEPTH: usize = 60;
const TREE_DEFAULT_MAX_ENTRIES: usize = 500;
const TREE_ABS_MAX_ENTRIES: usize = 10000;
fn rel_path_posix(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

/// 在 `abs_dir`（已位于工作区内）下列目录，按 glob 收集文件相对路径（相对**起始目录** `scan_root_display`）。
#[allow(clippy::too_many_arguments)] // 递归遍历需携带扫描上下文，字段多为路径/限制参数
fn walk_glob_collect_walkdir(
    scan_root: &Path,
    workspace_canonical: &Path,
    pattern: &Pattern,
    max_depth: usize,
    include_hidden: bool,
    max_results: usize,
    results: &mut Vec<String>,
) -> Result<(), String> {
    use walkdir::WalkDir;

    let walker = WalkDir::new(scan_root)
        .max_depth(max_depth + 1)
        .follow_links(false);

    for entry in walker {
        if results.len() >= max_results {
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.depth() == 0 {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if !include_hidden && name.starts_with('.') {
            continue;
        }
        if entry.file_type().is_dir() {
            continue;
        }
        let path = entry.path();
        if let Ok(canon) = path.canonicalize()
            && !canon.starts_with(workspace_canonical)
        {
            continue;
        }
        let rel = match path.strip_prefix(scan_root) {
            Ok(r) => rel_path_posix(r),
            Err(_) => continue,
        };
        if pattern.matches(&rel) {
            results.push(rel);
        }
    }
    Ok(())
}

/// 按 glob 模式递归查找工作区内文件路径（相对起始目录）。
/// 参数：`pattern`（必填，如 `**/*.rs`）、`path`（可选起始子目录，默认 `.`）、`max_depth`、`max_results`、`include_hidden`
pub fn glob_files(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let pattern_s = match v.get("pattern").and_then(|p| p.as_str()).map(str::trim) {
        Some(p) if !p.is_empty() => p,
        _ => return "错误：缺少 pattern 参数（glob，如 **/*.rs）".to_string(),
    };
    if pattern_s.starts_with('/') || pattern_s.contains("..") {
        return "错误：pattern 不能使用绝对路径或包含 ..".to_string();
    }
    let pattern = match Pattern::new(pattern_s) {
        Ok(p) => p,
        Err(e) => return format!("错误：glob 模式无效: {}", e),
    };

    let root = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    if root.starts_with('/') || root.contains("..") {
        return "错误：path 必须是工作区内的相对路径，且不能包含 .. 或绝对路径".to_string();
    }

    let max_depth = v
        .get("max_depth")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .unwrap_or(GLOB_DEFAULT_MAX_DEPTH)
        .clamp(0, GLOB_ABS_MAX_DEPTH);
    let max_results = v
        .get("max_results")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .unwrap_or(GLOB_DEFAULT_MAX_RESULTS)
        .clamp(1, GLOB_ABS_MAX_RESULTS);
    let include_hidden = v
        .get("include_hidden")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    let scan_root = match resolve_for_read(working_dir, root) {
        Ok(p) => p,
        Err(e) => return format!("错误：无法解析起始目录：{}", e),
    };
    if !scan_root.is_dir() {
        return format!(
            "错误：path 不是目录：{}",
            path_for_tool_display(working_dir, &scan_root, Some(root))
        );
    }
    let workspace_canonical = match canonical_workspace_root(working_dir) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };

    let mut results: Vec<String> = Vec::new();
    if let Err(e) = walk_glob_collect_walkdir(
        &scan_root,
        &workspace_canonical,
        &pattern,
        max_depth,
        include_hidden,
        max_results,
        &mut results,
    ) {
        return e;
    }

    results.sort();
    results.dedup();
    let truncated = results.len() >= max_results;
    let mut out = String::new();
    out.push_str(&format!(
        "起始目录（相对工作区）: {}\n模式: {}\nmax_depth={} max_results={} include_hidden={}\n---\n",
        root,
        pattern_s,
        max_depth,
        max_results,
        include_hidden
    ));
    for r in &results {
        out.push_str(r);
        out.push('\n');
    }
    out.push_str(&format!(
        "---\n匹配 {} 条路径{}",
        results.len(),
        if truncated {
            format!("（已达上限 {}，可能仍有未扫描到的匹配）", max_results)
        } else {
            String::new()
        }
    ));
    out
}

fn walk_list_tree_walkdir(
    scan_root: &Path,
    workspace_canonical: &Path,
    max_depth: usize,
    include_hidden: bool,
    max_entries: usize,
    lines: &mut Vec<(String, bool)>,
) -> Result<(), String> {
    use walkdir::WalkDir;

    let walker = WalkDir::new(scan_root)
        .max_depth(max_depth + 1)
        .sort_by(|a, b| {
            a.file_name()
                .to_string_lossy()
                .to_lowercase()
                .cmp(&b.file_name().to_string_lossy().to_lowercase())
        })
        .follow_links(false);

    for entry in walker {
        if lines.len() >= max_entries {
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.depth() == 0 {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if !include_hidden && name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if let Ok(canon) = path.canonicalize()
            && !canon.starts_with(workspace_canonical)
        {
            continue;
        }
        let is_dir = entry.file_type().is_dir();
        let rel = match path.strip_prefix(scan_root) {
            Ok(r) => rel_path_posix(r),
            Err(_) => continue,
        };
        lines.push((rel, is_dir));
    }
    Ok(())
}

/// 自起始目录起递归列出子路径（先序、字典序），含 `dir:` / `file:` 前缀。
/// 参数：`path`（可选，默认 `.`）、`max_depth`、`max_entries`、`include_hidden`
pub fn list_tree(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let root = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    if root.starts_with('/') || root.contains("..") {
        return "错误：path 必须是工作区内的相对路径，且不能包含 .. 或绝对路径".to_string();
    }
    let max_depth = v
        .get("max_depth")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .unwrap_or(TREE_DEFAULT_MAX_DEPTH)
        .clamp(0, TREE_ABS_MAX_DEPTH);
    let max_entries = v
        .get("max_entries")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .unwrap_or(TREE_DEFAULT_MAX_ENTRIES)
        .clamp(1, TREE_ABS_MAX_ENTRIES);
    let include_hidden = v
        .get("include_hidden")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    let scan_root = match resolve_for_read(working_dir, root) {
        Ok(p) => p,
        Err(e) => return format!("错误：无法解析起始目录：{}", e),
    };
    if !scan_root.is_dir() {
        return format!(
            "错误：path 不是目录：{}",
            path_for_tool_display(working_dir, &scan_root, Some(root))
        );
    }
    let workspace_canonical = match canonical_workspace_root(working_dir) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };

    let mut lines: Vec<(String, bool)> = Vec::new();
    lines.push((".".to_string(), true));
    if let Err(e) = walk_list_tree_walkdir(
        &scan_root,
        &workspace_canonical,
        max_depth,
        include_hidden,
        max_entries,
        &mut lines,
    ) {
        return e;
    }

    let truncated = lines.len() >= max_entries;
    let mut out = String::new();
    out.push_str(&format!(
        "起始目录（相对工作区）: {}\nmax_depth={} max_entries={} include_hidden={}\n---\n",
        root, max_depth, max_entries, include_hidden
    ));
    out.push_str("dir: .\n");
    for (rel, is_dir) in lines.iter().skip(1) {
        out.push_str(if *is_dir { "dir: " } else { "file: " });
        out.push_str(rel);
        if *is_dir && !rel.ends_with('/') {
            out.push('/');
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "---\n共 {} 条（含起点 .）{}",
        lines.len(),
        if truncated {
            format!("（已达上限 {}，树可能不完整）", max_entries)
        } else {
            String::new()
        }
    ));
    out
}
