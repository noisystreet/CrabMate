//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use std::path::Path;

use super::path::{
    parse_path_content, path_for_tool_display, resolve_for_read, resolve_for_write,
    tool_user_error_from_workspace_path,
};
use crate::tools::ToolContext;
use crate::tools::write_sse_preview::{
    WORKSPACE_WRITE_DIFF_BUDGET_CHARS, WriteDiffFileState,
    format_tool_output_with_write_diff_preview,
};
use crate::workspace::changelist::record_file_state_after_write;

/// 工具正文首行统一 `路径：…`，便于 Web 紧凑条与模型扫读（与 `run_command` 的 `命令：` 同理）。
#[inline]
fn tool_output_prepend_path(rel_display: &str, message: impl AsRef<str>) -> String {
    format!("路径：{}\n{}", rel_display.trim(), message.as_ref())
}

/// 复制/移动：`从→到：a → b` 与后续说明分行。
#[inline]
fn tool_output_prepend_from_to(from: &str, to: &str, message: impl AsRef<str>) -> String {
    format!(
        "从→到：{} → {}\n{}",
        from.trim(),
        to.trim(),
        message.as_ref()
    )
}

/// 创建文件：仅在文件不存在时创建；若已存在则报错。
/// 参数 args_json: { "path": string, "content": string }
pub fn create_file(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    let (path, content) = match parse_path_content(args_json) {
        Ok(pc) => pc,
        Err(e) => return e,
    };
    let target = match resolve_for_write(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    if target.exists() {
        return "错误：文件已存在，无法仅创建".to_string();
    }
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return format!("创建目录失败: {}", e);
    }
    match std::fs::write(&target, content.as_bytes()) {
        Ok(()) => {
            record_file_state_after_write(ctx.workspace_changelist, working_dir, &path, None);
            let disp = path_for_tool_display(working_dir, &target, Some(&path));
            let body = tool_output_prepend_path(&disp, format!("已创建文件: {}", disp));
            format_tool_output_with_write_diff_preview(
                "create_file",
                body,
                vec![WriteDiffFileState {
                    rel_path: path.clone(),
                    before: None,
                    after: Some(content.clone()),
                }],
                WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
            )
        }
        Err(e) => format!("写入文件失败: {}", e),
    }
}

#[cfg(unix)]
fn is_cross_device_rename(err: &std::io::Error) -> bool {
    err.raw_os_error() == Some(18) // EXDEV
}

#[cfg(windows)]
fn is_cross_device_rename(err: &std::io::Error) -> bool {
    err.raw_os_error() == Some(17) // ERROR_NOT_SAME_DEVICE
}

#[cfg(not(any(unix, windows)))]
fn is_cross_device_rename(_: &std::io::Error) -> bool {
    false
}

fn parse_from_to_overwrite(args_json: &str) -> Result<(String, String, bool), String> {
    let v: serde_json::Value = crate::tools::parse_args_json(args_json)?;
    let from = v
        .get("from")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "缺少 from（源相对路径）".to_string())?
        .to_string();
    let to = v
        .get("to")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "缺少 to（目标相对路径）".to_string())?
        .to_string();
    let overwrite = v
        .get("overwrite")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    Ok((from, to, overwrite))
}

fn check_dest_for_write_file(dst: &Path, overwrite: bool) -> Result<(), String> {
    if dst.exists() {
        if dst.is_dir() {
            return Err("错误：目标是已存在的目录，请指定具体文件路径".to_string());
        }
        if dst.is_file() && !overwrite {
            return Err("错误：目标文件已存在；若需覆盖请设置 overwrite 为 true".to_string());
        }
    }
    Ok(())
}

fn try_rename_or_move_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) => {
            if is_cross_device_rename(&e) {
                std::fs::copy(src, dst)?;
                std::fs::remove_file(src)?;
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}

/// 在工作区内复制**文件**（非目录）。源须已存在；路径规则与 `create_file` / `read_file` 相同（相对路径、`..` 与 symlink 逃逸校验）。
/// 参数：`from`、`to` 为相对工作目录路径；`overwrite` 可选，默认 `false`（目标已存在且为文件时须显式 `true` 才覆盖）。
pub fn copy_file(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    let (from, to, overwrite) = match parse_from_to_overwrite(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let src = match resolve_for_read(working_dir, &from) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    if !src.is_file() {
        return "错误：源路径不是常规文件（或为目录），仅支持复制文件".to_string();
    }
    let dst = match resolve_for_write(working_dir, &to) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    if src == dst {
        return "错误：源与目标解析后相同，无需复制".to_string();
    }
    if let Err(e) = check_dest_for_write_file(&dst, overwrite) {
        return e;
    }
    if let Some(parent) = dst.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return format!("创建目标父目录失败: {}", e);
    }
    match std::fs::copy(&src, &dst) {
        Ok(n) => {
            record_file_state_after_write(ctx.workspace_changelist, working_dir, &to, None);
            let body = tool_output_prepend_from_to(&from, &to, format!("已复制（{} 字节）", n));
            let after = std::fs::read_to_string(&dst).ok();
            format_tool_output_with_write_diff_preview(
                "copy_file",
                body,
                vec![WriteDiffFileState {
                    rel_path: to.clone(),
                    before: None,
                    after,
                }],
                WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
            )
        }
        Err(e) => format!("复制失败: {}", e),
    }
}

/// 在工作区内移动**文件**（重命名或迁路径）。`rename` 失败且为跨设备时自动回退为复制后删除源文件。
/// `overwrite` 默认 `false`：目标已存在为文件时须 `true` 才覆盖（与 `copy_file` 一致）。
pub fn move_file(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    let (from, to, overwrite) = match parse_from_to_overwrite(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let src = match resolve_for_read(working_dir, &from) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    if !src.is_file() {
        return "错误：源路径不是常规文件（或为目录），仅支持移动文件".to_string();
    }
    let dst = match resolve_for_write(working_dir, &to) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    if src == dst {
        return "错误：源与目标解析后相同".to_string();
    }
    if let Err(e) = check_dest_for_write_file(&dst, overwrite) {
        return e;
    }
    if let Some(parent) = dst.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return format!("创建目标父目录失败: {}", e);
    }
    match try_rename_or_move_file(&src, &dst) {
        Ok(()) => {
            record_file_state_after_write(ctx.workspace_changelist, working_dir, &from, None);
            record_file_state_after_write(ctx.workspace_changelist, working_dir, &to, None);
            tool_output_prepend_from_to(&from, &to, "已移动")
        }
        Err(e) => format!("移动失败: {}", e),
    }
}

/// 修改文件：仅在文件已存在时写入。
/// - 默认 `mode`=`full`：整文件覆盖（`content` 为全文）。
/// - `mode`=`replace_lines`：`start_line`..=`end_line`（1-based，含边界）替换为 `content`（流式读写，适合大文件）。
pub fn modify_file(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let path = match v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(p) => p.to_string(),
        None => return "缺少 path 参数".to_string(),
    };

    let mode = v
        .get("mode")
        .and_then(|m| m.as_str())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| "full".to_string());

    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    if !target.is_file() {
        return "错误：路径不是文件或不存在，无法仅修改".to_string();
    }

    if mode == "replace_lines" || mode == "lines" {
        let display = path_for_tool_display(working_dir, &target, Some(&path));
        super::replace_lines_stream::modify_file_replace_lines(
            &v,
            &target,
            &display,
            ctx,
            working_dir,
            &path,
        )
    } else if mode == "full" || mode.is_empty() {
        let content = v
            .get("content")
            .and_then(|c| c.as_str())
            .map(String::from)
            .unwrap_or_default();
        let before = std::fs::read_to_string(&target).ok();
        match std::fs::write(&target, content.as_bytes()) {
            Ok(()) => {
                let before_preview = before.clone();
                record_file_state_after_write(ctx.workspace_changelist, working_dir, &path, before);
                let disp = path_for_tool_display(working_dir, &target, Some(&path));
                let body = tool_output_prepend_path(&disp, format!("已整文件覆盖: {}", disp));
                let after = std::fs::read_to_string(&target).ok();
                format_tool_output_with_write_diff_preview(
                    "modify_file",
                    body,
                    vec![WriteDiffFileState {
                        rel_path: path.clone(),
                        before: before_preview,
                        after,
                    }],
                    WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
                )
            }
            Err(e) => format!("写入文件失败: {}", e),
        }
    } else {
        format!("错误：mode 仅支持 full 或 replace_lines（收到 {:?}）", mode)
    }
}
