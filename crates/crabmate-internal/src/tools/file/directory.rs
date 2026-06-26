//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use std::cmp::Ordering;
use std::path::Path;

use super::display_fmt::{format_size, format_unix_timestamp};
use super::path::{path_for_tool_display, resolve_for_read};

fn prepend_read_dir_output_header(
    body: &str,
    path_disp: &str,
    entries_shown: usize,
    entries_walked: usize,
) -> String {
    let header = serde_json::json!({
        "kind": "crabmate_tool_output",
        "tool": "read_dir",
        "version": 1,
        "path": path_disp,
        "entries_shown": entries_shown,
        "entries_walked": entries_walked,
    });
    format!("{}\n{}", header, body)
}

/// 敏感路径前缀（拒绝绝对路径访问）
const SENSITIVE_EXTERNAL_PATHS: &[&str] = &["/proc", "/sys", "/dev", "/root", "/etc"];

fn is_sensitive_external_path(path: &str) -> bool {
    SENSITIVE_EXTERNAL_PATHS
        .iter()
        .any(|s| path == *s || path.starts_with(&format!("{}/", s)))
}

struct DirEntry {
    name: String,
    is_dir: bool,
    size: u64,
    mtime: Option<std::time::SystemTime>,
}

fn cmp_dir_entries(a: &DirEntry, b: &DirEntry, sort_by: &str) -> Ordering {
    match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => match sort_by {
            "size" => b.size.cmp(&a.size),
            "mtime" => b.mtime.cmp(&a.mtime),
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        },
    }
}

fn collect_read_dir_entries(rd: std::fs::ReadDir, include_hidden: bool) -> (Vec<DirEntry>, usize) {
    let mut count = 0usize;
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
    (entries, count)
}

fn format_dir_entry_line(entry: &DirEntry, include_size: bool, include_mtime: bool) -> String {
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
    line
}

fn resolve_read_dir_root(path: &str, working_dir: &Path) -> Result<std::path::PathBuf, String> {
    if path.starts_with('/') {
        if is_sensitive_external_path(path) {
            return Err(format!("错误：禁止访问敏感系统路径：{}", path));
        }
        Ok(Path::new(path).to_path_buf())
    } else if path.contains("..") {
        Err("错误：path 必须是工作区内的相对路径，且不能包含 .. 或绝对路径".to_string())
    } else {
        resolve_for_read(working_dir, path).map_err(|e| format!("错误：无法解析目录路径：{}", e))
    }
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

    let root = match resolve_read_dir_root(path, working_dir) {
        Ok(p) => p,
        Err(msg) => return msg,
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
            let (mut entries, count) = collect_read_dir_entries(rd, include_hidden);
            entries.sort_by(|a, b| cmp_dir_entries(a, b, sort_by));
            let mut shown = 0usize;
            for entry in entries.into_iter().take(max_entries) {
                shown += 1;
                let line = format_dir_entry_line(&entry, include_size, include_mtime);
                out.push_str(&line);
                out.push('\n');
            }
            out.push_str(&format!("总计遍历: {}，展示: {}\n", count, shown));
            let path_disp = path_for_tool_display(working_dir, &root, Some(path));
            prepend_read_dir_output_header(out.trim_end(), &path_disp, shown, count)
        }
        Err(e) => format!("读取目录失败：{}", e),
    }
}
