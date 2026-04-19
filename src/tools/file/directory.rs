//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use std::path::Path;

use super::display_fmt::{format_size, format_unix_timestamp};
use super::path::{path_for_tool_display, resolve_for_read};

/// 敏感路径前缀（拒绝绝对路径访问）
const SENSITIVE_EXTERNAL_PATHS: &[&str] = &["/proc", "/sys", "/dev", "/root", "/etc"];

fn is_sensitive_external_path(path: &str) -> bool {
    SENSITIVE_EXTERNAL_PATHS
        .iter()
        .any(|s| path == *s || path.starts_with(&format!("{}/", s)))
}

pub fn read_dir(args_json: &str, working_dir: &Path) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(".");

    let root = if path.starts_with('/') {
        // 绝对路径（外部路径，已在 tool_registry 层审批）
        if is_sensitive_external_path(path) {
            return format!("错误：禁止访问敏感系统路径：{}", path);
        }
        Path::new(path).to_path_buf()
    } else if path.contains("..") {
        return "错误：path 必须是工作区内的相对路径，且不能包含 .. 或绝对路径".to_string();
    } else {
        // 相对路径，正常处理
        match resolve_for_read(working_dir, path) {
            Ok(p) => p,
            Err(e) => return format!("错误：无法解析目录路径：{}", e),
        }
    };

    if !root.is_dir() {
        return format!(
            "错误：指定路径不是目录：{}",
            path_for_tool_display(working_dir, &root, Some(path))
        );
    }

    let max_entries = v
        .get("max_entries")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(200);
    let include_hidden = v
        .get("include_hidden")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    let include_size = v
        .get("include_size")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    let include_mtime = v
        .get("include_mtime")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    let sort_by = v
        .get("sort_by")
        .and_then(|s| s.as_str())
        .map(str::trim)
        .unwrap_or("name");

    let mut out = String::new();
    out.push_str(&format!(
        "目录: {}\n",
        path_for_tool_display(working_dir, &root, Some(path))
    ));
    match std::fs::read_dir(&root) {
        Ok(rd) => {
            let mut count = 0usize;
            let mut shown = 0usize;
            struct DirEntry {
                name: String,
                is_dir: bool,
                size: u64,
                mtime: Option<std::time::SystemTime>,
            }
            let mut entries: Vec<DirEntry> = Vec::new();
            for e in rd.flatten() {
                count += 1;
                let name = e.file_name().to_string_lossy().to_string();
                if !include_hidden && name.starts_with('.') {
                    continue;
                }
                let meta = e.metadata().ok();
                let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let mtime = meta.as_ref().and_then(|m| m.modified().ok());
                entries.push(DirEntry {
                    name,
                    is_dir,
                    size,
                    mtime,
                });
            }
            entries.sort_by(|a, b| {
                let dir_ord = match (a.is_dir, b.is_dir) {
                    (true, false) => return std::cmp::Ordering::Less,
                    (false, true) => return std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                };
                let _ = dir_ord;
                match sort_by {
                    "size" => b.size.cmp(&a.size),
                    "mtime" => b.mtime.cmp(&a.mtime),
                    _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                }
            });
            for entry in entries.into_iter().take(max_entries) {
                shown += 1;
                let prefix = if entry.is_dir { "dir: " } else { "file: " };
                let mut line = format!("{}{}", prefix, entry.name);
                if include_size && !entry.is_dir {
                    line.push_str(&format!("  ({})", format_size(entry.size)));
                }
                if include_mtime
                    && let Some(mt) = entry.mtime
                    && let Ok(dur) = mt.duration_since(std::time::UNIX_EPOCH)
                {
                    let dt = format_unix_timestamp(dur.as_secs());
                    line.push_str(&format!("  [{}]", dt));
                }
                out.push_str(&line);
                out.push('\n');
            }
            out.push_str(&format!("总计遍历: {}，展示: {}\n", count, shown));
            out.trim_end().to_string()
        }
        Err(e) => format!("读取目录失败：{}", e),
    }
}
