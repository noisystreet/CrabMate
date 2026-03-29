//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use regex::RegexBuilder;
use std::io::Write;
use std::path::Path;

use super::path::{
    canonical_workspace_root, path_for_tool_display, resolve_for_read, resolve_for_write,
};
use crate::tools::ToolContext;
use crate::workspace_changelist::record_file_state_after_write;

pub fn delete_file(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v.get("path").and_then(|p| p.as_str()).map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "缺少 path 参数".to_string(),
    };
    let confirm = v.get("confirm").and_then(|c| c.as_bool()).unwrap_or(false);
    if !confirm {
        return "拒绝执行：delete_file 需要 confirm=true".to_string();
    }

    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !target.is_file() {
        return format!(
            "错误：{} 不是文件（可能是目录，请用 delete_dir）",
            path_for_tool_display(working_dir, &target, Some(&path))
        );
    }
    let before = std::fs::read_to_string(&target).ok();
    match std::fs::remove_file(&target) {
        Ok(()) => {
            if let Some(c) = ctx.workspace_changelist {
                c.record_mutation(&path, before, None);
            }
            format!(
                "已删除文件：{}",
                path_for_tool_display(working_dir, &target, Some(&path))
            )
        }
        Err(e) => format!("删除文件失败：{}", e),
    }
}

// ── delete_dir ──────────────────────────────────────────────

pub fn delete_dir(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v.get("path").and_then(|p| p.as_str()).map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "缺少 path 参数".to_string(),
    };
    let confirm = v.get("confirm").and_then(|c| c.as_bool()).unwrap_or(false);
    if !confirm {
        return "拒绝执行：delete_dir 需要 confirm=true".to_string();
    }
    let recursive = v
        .get("recursive")
        .and_then(|r| r.as_bool())
        .unwrap_or(false);

    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !target.is_dir() {
        return format!(
            "错误：{} 不是目录",
            path_for_tool_display(working_dir, &target, Some(&path))
        );
    }
    let base_canonical = match canonical_workspace_root(working_dir) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if target == base_canonical {
        return "错误：不能删除工作区根目录".to_string();
    }

    let result = if recursive {
        std::fs::remove_dir_all(&target)
    } else {
        std::fs::remove_dir(&target)
    };
    match result {
        Ok(()) => format!(
            "已删除目录{}：{}",
            if recursive { "（递归）" } else { "" },
            path_for_tool_display(working_dir, &target, Some(&path))
        ),
        Err(e) => {
            if !recursive && e.kind() == std::io::ErrorKind::DirectoryNotEmpty {
                "删除失败：目录非空，需要 recursive=true 才能删除非空目录".to_string()
            } else {
                format!("删除目录失败：{}", e)
            }
        }
    }
}

// ── append_file ─────────────────────────────────────────────

pub fn append_file(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v.get("path").and_then(|p| p.as_str()).map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "缺少 path 参数".to_string(),
    };
    let content = v
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or_default();
    let create_if_missing = v
        .get("create_if_missing")
        .and_then(|c| c.as_bool())
        .unwrap_or(false);

    let target = if create_if_missing {
        match resolve_for_write(working_dir, &path) {
            Ok(p) => p,
            Err(e) => return e,
        }
    } else {
        match resolve_for_read(working_dir, &path) {
            Ok(p) => p,
            Err(e) => return format!("文件不存在（可设置 create_if_missing=true）：{}", e),
        }
    };

    if create_if_missing
        && let Some(parent) = target.parent()
        && !parent.exists()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return format!("创建父目录失败：{}", e);
    }

    let before = std::fs::read_to_string(&target).ok();
    let mut file = match std::fs::OpenOptions::new()
        .append(true)
        .create(create_if_missing)
        .open(&target)
    {
        Ok(f) => f,
        Err(e) => return format!("打开文件失败：{}", e),
    };
    match file.write_all(content.as_bytes()) {
        Ok(()) => {
            if let Some(c) = ctx.workspace_changelist {
                c.record_mutation(&path, before, std::fs::read_to_string(&target).ok());
            }
            format!(
                "已追加 {} 字节到 {}",
                content.len(),
                path_for_tool_display(working_dir, &target, Some(&path))
            )
        }
        Err(e) => format!("写入失败：{}", e),
    }
}

// ── create_dir ──────────────────────────────────────────────

pub fn create_dir(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v.get("path").and_then(|p| p.as_str()).map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "缺少 path 参数".to_string(),
    };
    let parents = v.get("parents").and_then(|p| p.as_bool()).unwrap_or(true);

    let target = match resolve_for_write(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if target.exists() {
        if target.is_dir() {
            return format!(
                "目录已存在：{}",
                path_for_tool_display(working_dir, &target, Some(&path))
            );
        }
        return format!(
            "错误：路径已存在且为文件：{}",
            path_for_tool_display(working_dir, &target, Some(&path))
        );
    }
    let result = if parents {
        std::fs::create_dir_all(&target)
    } else {
        std::fs::create_dir(&target)
    };
    match result {
        Ok(()) => format!(
            "已创建目录：{}",
            path_for_tool_display(working_dir, &target, Some(&path))
        ),
        Err(e) => format!("创建目录失败：{}", e),
    }
}

// ── search_replace ──────────────────────────────────────────

pub fn search_replace(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v.get("path").and_then(|p| p.as_str()).map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "缺少 path 参数".to_string(),
    };
    let search = match v.get("search").and_then(|s| s.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "缺少 search 参数".to_string(),
    };
    let replace = v
        .get("replace")
        .and_then(|r| r.as_str())
        .unwrap_or("")
        .to_string();
    let is_regex = v.get("regex").and_then(|r| r.as_bool()).unwrap_or(false);
    let dry_run = v.get("dry_run").and_then(|d| d.as_bool()).unwrap_or(true);
    let confirm = v.get("confirm").and_then(|c| c.as_bool()).unwrap_or(false);
    let max_replacements = v
        .get("max_replacements")
        .and_then(|m| m.as_u64())
        .unwrap_or(0) as usize;

    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !target.is_file() {
        return format!("错误：{} 不是文件", path);
    }

    let content = match std::fs::read_to_string(&target) {
        Ok(c) => c,
        Err(e) => return format!("读取文件失败：{}", e),
    };
    const MAX_FILE_SIZE: usize = 4 * 1024 * 1024;
    if content.len() > MAX_FILE_SIZE {
        return format!("错误：文件过大（{} 字节，上限 4MiB）", content.len());
    }

    let (new_content, count) = if is_regex {
        let re = match RegexBuilder::new(&search).build() {
            Ok(r) => r,
            Err(e) => return format!("正则表达式无效：{}", e),
        };
        let mut count = 0usize;
        let new = if max_replacements == 0 {
            let result = re.replace_all(&content, replace.as_str());
            count = re.find_iter(&content).count();
            result.to_string()
        } else {
            let mut result = content.clone();
            for _ in 0..max_replacements {
                if let Some(m) = re.find(&result) {
                    let before = &result[..m.start()];
                    let after = &result[m.end()..];
                    result = format!("{}{}{}", before, replace, after);
                    count += 1;
                } else {
                    break;
                }
            }
            result
        };
        (new, count)
    } else {
        let mut count = 0usize;
        let new = if max_replacements == 0 {
            count = content.matches(&search).count();
            content.replace(&search, &replace)
        } else {
            let mut result = content.clone();
            for _ in 0..max_replacements {
                if let Some(pos) = result.find(&search) {
                    result = format!(
                        "{}{}{}",
                        &result[..pos],
                        replace,
                        &result[pos + search.len()..]
                    );
                    count += 1;
                } else {
                    break;
                }
            }
            result
        };
        (new, count)
    };

    if count == 0 {
        return format!("未找到匹配：\"{}\" 在 {}", search, path);
    }

    let display = path_for_tool_display(working_dir, &target, Some(&path));
    if dry_run {
        let mut preview = format!("预览（dry-run）：在 {} 中找到 {} 处匹配\n", display, count);
        let lines: Vec<&str> = new_content.lines().collect();
        let orig_lines: Vec<&str> = content.lines().collect();
        let mut shown = 0usize;
        for (i, (old, new)) in orig_lines.iter().zip(lines.iter()).enumerate() {
            if old != new && shown < 20 {
                preview.push_str(&format!(
                    "  L{}: \"{}\" → \"{}\"\n",
                    i + 1,
                    old.trim(),
                    new.trim()
                ));
                shown += 1;
            }
        }
        if shown >= 20 {
            preview.push_str("  ... (更多变更已省略)\n");
        }
        preview.push_str("\n设置 dry_run=false, confirm=true 以实际写入");
        return preview;
    }

    if !confirm {
        return "拒绝执行：search_replace 写盘需要 confirm=true".to_string();
    }

    let before = content.clone();
    match std::fs::write(&target, new_content.as_bytes()) {
        Ok(()) => {
            record_file_state_after_write(
                ctx.workspace_changelist,
                working_dir,
                &path,
                Some(before),
            );
            format!(
                "已替换 {} 处匹配（\"{}\" → \"{}\"）：{}",
                count, search, replace, display
            )
        }
        Err(e) => format!("写入文件失败：{}", e),
    }
}
