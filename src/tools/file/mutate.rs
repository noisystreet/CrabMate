//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use regex::RegexBuilder;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::path::{
    canonical_workspace_root, path_for_tool_display, resolve_for_read, resolve_for_write,
    tool_user_error_from_workspace_path,
};
use crate::tools::ToolContext;
use crate::tools::tool_param_types::{
    AppendFileArgs, CreateDirArgs, DeleteDirArgs, DeleteFileArgs, SearchReplaceArgs,
};
use crate::tools::write_sse_preview::{
    WORKSPACE_WRITE_DIFF_BUDGET_CHARS, WriteDiffFileState,
    format_tool_output_with_write_diff_preview,
};
use crate::workspace::changelist::record_file_state_after_write;

pub fn delete_file(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: DeleteFileArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数解析错误: {e}"),
    };
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return "缺少 path 参数".to_string(),
    };
    let confirm = args.confirm.unwrap_or(false);
    if !confirm {
        return "拒绝执行：delete_file 需要 confirm=true".to_string();
    }

    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
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
            let body = format!(
                "已删除文件：{}",
                path_for_tool_display(working_dir, &target, Some(&path))
            );
            let preview_before = before.clone();
            if let Some(c) = ctx.workspace_changelist {
                c.record_mutation(&path, before, None);
            }
            format_tool_output_with_write_diff_preview(
                "delete_file",
                body,
                vec![WriteDiffFileState {
                    rel_path: path.clone(),
                    before: preview_before,
                    after: None,
                }],
                WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
            )
        }
        Err(e) => format!("删除文件失败：{}", e),
    }
}

// ── delete_dir ──────────────────────────────────────────────

pub fn delete_dir(args_json: &str, working_dir: &Path) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: DeleteDirArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数解析错误: {e}"),
    };
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return "缺少 path 参数".to_string(),
    };
    let confirm = args.confirm.unwrap_or(false);
    if !confirm {
        return "拒绝执行：delete_dir 需要 confirm=true".to_string();
    }
    let recursive = args.recursive;

    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    if !target.is_dir() {
        return format!(
            "错误：{} 不是目录",
            path_for_tool_display(working_dir, &target, Some(&path))
        );
    }
    let base_canonical = match canonical_workspace_root(working_dir) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
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
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: AppendFileArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数解析错误: {e}"),
    };
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return "缺少 path 参数".to_string(),
    };
    let content = args.content;
    let create_if_missing = args.create_if_missing;

    let target = if create_if_missing {
        match resolve_for_write(working_dir, &path) {
            Ok(p) => p,
            Err(e) => return tool_user_error_from_workspace_path(e),
        }
    } else {
        match resolve_for_read(working_dir, &path) {
            Ok(p) => p,
            Err(e) => {
                return format!("文件不存在（可设置 create_if_missing=true）：{}", e);
            }
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
            let after = std::fs::read_to_string(&target).ok();
            if let Some(c) = ctx.workspace_changelist {
                c.record_mutation(&path, before.clone(), after.clone());
            }
            let body = format!(
                "已追加 {} 字节到 {}",
                content.len(),
                path_for_tool_display(working_dir, &target, Some(&path))
            );
            format_tool_output_with_write_diff_preview(
                "append_file",
                body,
                vec![WriteDiffFileState {
                    rel_path: path.clone(),
                    before,
                    after,
                }],
                WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
            )
        }
        Err(e) => format!("写入失败：{}", e),
    }
}

// ── create_dir ──────────────────────────────────────────────

pub fn create_dir(args_json: &str, working_dir: &Path) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: CreateDirArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数解析错误: {e}"),
    };
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return "缺少 path 参数".to_string(),
    };
    let parents = args.parents;

    let target = match resolve_for_write(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
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

fn apply_search_replace_inner(
    content: &str,
    search: &str,
    replace: &str,
    is_regex: bool,
    max_replacements: usize,
) -> Result<(String, usize), String> {
    if is_regex {
        let re = RegexBuilder::new(search)
            .build()
            .map_err(|e| format!("正则表达式无效：{}", e))?;
        let mut count = 0usize;
        let new = if max_replacements == 0 {
            let result = re.replace_all(content, replace);
            count = re.find_iter(content).count();
            result.to_string()
        } else {
            let mut result = content.to_string();
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
        Ok((new, count))
    } else {
        let mut count = 0usize;
        let new = if max_replacements == 0 {
            count = content.matches(search).count();
            content.replace(search, replace)
        } else {
            let mut result = content.to_string();
            for _ in 0..max_replacements {
                if let Some(pos) = result.find(search) {
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
        Ok((new, count))
    }
}

fn search_replace_dry_run_preview(
    display: &str,
    count: usize,
    content: &str,
    new_content: &str,
) -> String {
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
    preview
}

fn parse_search_replace_args(args_json: &str) -> Result<SearchReplaceArgs, String> {
    let v = crate::tools::parse_args_json(args_json)?;
    serde_json::from_value(v).map_err(|e| format!("参数解析错误: {e}"))
}

fn search_replace_path_and_query(args: &SearchReplaceArgs) -> Result<(String, String), String> {
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return Err("缺少 path 参数".to_string()),
    };
    let search = match args.search.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return Err("缺少 search 参数".to_string()),
    };
    Ok((path, search))
}

fn load_search_replace_file_bytes(
    working_dir: &Path,
    path: &str,
) -> Result<(PathBuf, String), String> {
    let target =
        resolve_for_read(working_dir, path).map_err(tool_user_error_from_workspace_path)?;
    if !target.is_file() {
        return Err(format!("错误：{} 不是文件", path));
    }
    let content = std::fs::read_to_string(&target).map_err(|e| format!("读取文件失败：{}", e))?;
    const MAX_FILE_SIZE: usize = 4 * 1024 * 1024;
    if content.len() > MAX_FILE_SIZE {
        return Err(format!(
            "错误：文件过大（{} 字节，上限 4MiB）",
            content.len()
        ));
    }
    Ok((target, content))
}

pub fn search_replace(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    let args = match parse_search_replace_args(args_json) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let (path, search) = match search_replace_path_and_query(&args) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let replace = args.replace;
    let is_regex = args.regex;
    let dry_run = args.dry_run;
    let confirm = args.confirm;
    let max_replacements = args.max_replacements.unwrap_or(0) as usize;

    let (target, content) = match load_search_replace_file_bytes(working_dir, &path) {
        Ok(x) => x,
        Err(e) => return e,
    };

    let (new_content, count) =
        match apply_search_replace_inner(&content, &search, &replace, is_regex, max_replacements) {
            Ok(x) => x,
            Err(e) => return e,
        };

    if count == 0 {
        return format!("未找到匹配：\"{}\" 在 {}", search, path);
    }

    let display = path_for_tool_display(working_dir, &target, Some(&path));
    if dry_run {
        return search_replace_dry_run_preview(&display, count, &content, &new_content);
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
                Some(before.clone()),
            );
            let body = format!(
                "已替换 {} 处匹配（\"{}\" → \"{}\"）：{}",
                count, search, replace, display
            );
            format_tool_output_with_write_diff_preview(
                "search_replace",
                body,
                vec![WriteDiffFileState {
                    rel_path: path,
                    before: Some(before),
                    after: Some(new_content),
                }],
                WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
            )
        }
        Err(e) => format!("写入文件失败：{}", e),
    }
}
