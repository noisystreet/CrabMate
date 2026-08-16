//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use std::path::Path;

use super::path::{
    canonical_workspace_root, parse_path_content, path_for_tool_display, resolve_for_read,
    resolve_for_read_open, resolve_for_write, tool_user_error_from_workspace_path,
};
use crate::cm_tools::tools::ToolContext;
use crate::cm_tools::tools::write_sse_preview::{
    WORKSPACE_WRITE_DIFF_BUDGET_CHARS, WriteDiffFileState,
    format_tool_output_with_write_diff_preview,
};
use crate::cm_tools::workspace::changelist::record_file_state_after_write;
use crate::cm_tools::workspace::fs::{
    copy_opened_file_under_root, rename_file_under_root, write_bytes_under_root,
};

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
    let base = match canonical_workspace_root(working_dir) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    let target = match resolve_for_write(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    if target.exists() {
        return "错误：文件已存在，无法仅创建".to_string();
    }
    match write_bytes_under_root(&base, &target, content.as_bytes(), true, false) {
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

fn parse_from_to_overwrite(args_json: &str) -> Result<(String, String, bool), String> {
    let v: serde_json::Value = crate::cm_tools::tools::parse_args_json(args_json)?;
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

/// 在工作区内复制**文件**（非目录）。源须已存在；路径规则与 `create_file` / `read_file` 相同（相对路径、`..` 与 symlink 逃逸校验）。
/// 参数：`from`、`to` 为相对工作目录路径；`overwrite` 可选，默认 `false`（目标已存在且为文件时须显式 `true` 才覆盖）。
pub fn copy_file(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    let (from, to, overwrite) = match parse_from_to_overwrite(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let base = match canonical_workspace_root(working_dir) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    let opened = match resolve_for_read_open(working_dir, &from) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    if !opened.file.metadata().map(|m| m.is_file()).unwrap_or(false) {
        return "错误：源路径不是常规文件（或为目录），仅支持复制文件".to_string();
    }
    let dst = match resolve_for_write(working_dir, &to) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    if opened.resolved_path == dst {
        return "错误：源与目标解析后相同，无需复制".to_string();
    }
    if let Err(e) = check_dest_for_write_file(&dst, overwrite) {
        return e;
    }
    match copy_opened_file_under_root(&base, &opened, &dst, overwrite) {
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
    let base = match canonical_workspace_root(working_dir) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
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
    match rename_file_under_root(&base, &src, &dst) {
        Ok(()) => {
            record_file_state_after_write(ctx.workspace_changelist, working_dir, &from, None);
            record_file_state_after_write(ctx.workspace_changelist, working_dir, &to, None);
            tool_output_prepend_from_to(&from, &to, "已移动")
        }
        Err(e) => format!("移动失败: {}", e),
    }
}

fn full_overwrite_shrink_heuristic_warn(old_b: usize, new_b: usize) -> bool {
    if old_b < 400 {
        return false;
    }
    if new_b == 0 {
        return old_b >= 200;
    }
    new_b.saturating_mul(4) < old_b
}

/// 整文件覆盖前须用户显式确认的高危情形（与字节缩短启发式 `full_overwrite_shrink_heuristic_warn` 叠加）。
fn full_overwrite_requires_user_confirm(before: &str, content: &str) -> bool {
    let old_b = before.len();
    let new_b = content.len();
    if full_overwrite_shrink_heuristic_warn(old_b, new_b) {
        return true;
    }
    if !before.is_empty() && content.is_empty() {
        return true;
    }
    let old_lines = before.lines().count().max(1);
    let new_lines = content.lines().count().max(1);
    old_lines >= 30 && new_lines.saturating_mul(3) < old_lines
}

fn modify_file_write_full_overwrite(
    path: &str,
    target: &Path,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
    content: String,
    dry_run: bool,
    confirm_full_overwrite: bool,
) -> String {
    let before = std::fs::read_to_string(target).ok();
    let before_str = before.as_deref().unwrap_or("");
    let disp = path_for_tool_display(working_dir, target, Some(path));

    if dry_run {
        let body = tool_output_prepend_path(
            &disp,
            "预览（dry_run=true）：整文件覆盖未写盘。设置 dry_run=false 以执行；若工具提示须确认，再附带 confirm_full_overwrite=true。",
        );
        return format_tool_output_with_write_diff_preview(
            "modify_file",
            body,
            vec![WriteDiffFileState {
                rel_path: path.to_string(),
                before: before.clone(),
                after: Some(content.clone()),
            }],
            WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
        );
    }

    if full_overwrite_requires_user_confirm(before_str, &content) && !confirm_full_overwrite {
        let body = tool_output_prepend_path(
            &disp,
            "错误：本次整文件覆盖将大幅缩短、删去大量行或清空非空文件。**未写盘**。\
下方为本次写入的 diff 预览；确认后使用 confirm_full_overwrite=true 再次调用，或改用 mode=replace_lines / mode=insert_after_line / search_replace 做局部修改。",
        );
        return format_tool_output_with_write_diff_preview(
            "modify_file",
            body,
            vec![WriteDiffFileState {
                rel_path: path.to_string(),
                before: before.clone(),
                after: Some(content.clone()),
            }],
            WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
        );
    }

    let base = match canonical_workspace_root(working_dir) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    match write_bytes_under_root(&base, target, content.as_bytes(), false, false) {
        Ok(()) => {
            let before_preview = before.clone();
            record_file_state_after_write(ctx.workspace_changelist, working_dir, path, before);
            let old_b = before_preview.as_ref().map(String::len).unwrap_or(0);
            let shrink_warn = full_overwrite_shrink_heuristic_warn(old_b, content.len());
            let mut body = tool_output_prepend_path(&disp, format!("已整文件覆盖: {}", disp));
            if shrink_warn {
                body.push_str(
                    "\n\n警告：新正文显著短于原文件，疑似未传完整全文（整文件覆盖不可逆）。\
若非有意清空/大幅缩短，请尽快用 Git 或备份恢复原内容；后续局部改动请使用 mode=replace_lines。",
                );
            }
            let after = std::fs::read_to_string(target).ok();
            format_tool_output_with_write_diff_preview(
                "modify_file",
                body,
                vec![WriteDiffFileState {
                    rel_path: path.to_string(),
                    before: before_preview,
                    after,
                }],
                WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
            )
        }
        Err(e) => format!("写入文件失败: {}", e),
    }
}

fn modify_file_path_arg(v: &serde_json::Value) -> Result<String, String> {
    v.get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "缺少 path 参数".to_string())
}

fn modify_file_mode_arg(v: &serde_json::Value) -> (Option<&str>, String) {
    let explicit_mode = v.get("mode").and_then(|m| m.as_str());
    let mode = explicit_mode
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| {
            if v.get("start_line").is_some() || v.get("end_line").is_some() {
                "replace_lines".to_string()
            } else if v.get("after_line").is_some() {
                "insert_after_line".to_string()
            } else {
                "full".to_string()
            }
        });
    (explicit_mode, mode)
}

fn modify_file_display(working_dir: &Path, target: &Path, path: &str) -> String {
    path_for_tool_display(working_dir, target, Some(path))
}

fn modify_file_dispatch_local_mode(
    v: &serde_json::Value,
    target: &Path,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
    path: &str,
    mode: &str,
) -> Option<String> {
    let display = modify_file_display(working_dir, target, path);
    match mode {
        "replace_lines" | "lines" | "replacelines" => {
            Some(super::replace_lines_stream::modify_file_replace_lines(
                v,
                target,
                &display,
                ctx,
                working_dir,
                path,
            ))
        }
        "insert_after_line" => Some(super::replace_lines_stream::modify_file_insert_after_line(
            v,
            target,
            &display,
            ctx,
            working_dir,
            path,
        )),
        _ => None,
    }
}

fn modify_file_dispatch_full_mode(
    v: &serde_json::Value,
    target: &Path,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
    path: &str,
    explicit_mode: Option<&str>,
) -> String {
    if (v.get("start_line").is_some() || v.get("end_line").is_some()) && explicit_mode.is_some() {
        return "错误：mode=full/overwrite 是整文件覆盖，不能与 start_line/end_line 混用；局部修改请使用 mode=replace_lines。"
            .to_string();
    }
    let content = v
        .get("content")
        .and_then(|c| c.as_str())
        .map(String::from)
        .unwrap_or_default();
    let dry_run = v.get("dry_run").and_then(|x| x.as_bool()).unwrap_or(false);
    let confirm_full_overwrite = v
        .get("confirm_full_overwrite")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    modify_file_write_full_overwrite(
        path,
        target,
        working_dir,
        ctx,
        content,
        dry_run,
        confirm_full_overwrite,
    )
}

fn modify_file_dispatch_mode(
    v: &serde_json::Value,
    target: &Path,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
    path: &str,
    explicit_mode: Option<&str>,
    mode: &str,
) -> String {
    if let Some(out) = modify_file_dispatch_local_mode(v, target, working_dir, ctx, path, mode) {
        return out;
    }
    if mode == "full" || mode == "overwrite" || mode.is_empty() {
        return modify_file_dispatch_full_mode(v, target, working_dir, ctx, path, explicit_mode);
    }
    format!(
        "错误：mode 仅支持 full、overwrite、replace_lines 或 insert_after_line（收到 {mode:?}）"
    )
}

/// 修改文件：仅在文件已存在时写入。
/// - 未显式给出 `mode` 但带有 `start_line`/`end_line` 时自动使用 `replace_lines`，带有 `after_line` 时自动使用 `insert_after_line`；否则默认 `mode`=`full`。
/// - `mode`=`replace_lines`：`start_line`..=`end_line`（1-based，含边界）替换为 `content`（流式读写，适合大文件）。
/// - `mode`=`insert_after_line`：在 `after_line` 之后插入 `content`；`after_line=0` 表示文件开头。
/// - `dry_run=true`：仅返回 diff 预览，不写盘（`replace_lines` 与整文件覆盖均支持）。
/// - 整文件覆盖若命中高危启发式，须 `confirm_full_overwrite=true`（`dry_run` 不受此限）。
pub fn modify_file(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    let v = match crate::cm_tools::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let path = match modify_file_path_arg(&v) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let (explicit_mode, mode) = modify_file_mode_arg(&v);

    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    if !target.is_file() {
        return "错误：路径不是文件或不存在，无法仅修改".to_string();
    }

    modify_file_dispatch_mode(&v, &target, working_dir, ctx, &path, explicit_mode, &mode)
}
