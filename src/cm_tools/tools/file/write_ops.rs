//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use std::path::Path;

use super::path::{
    canonical_workspace_root, parse_path_content, path_for_tool_display, resolve_for_read,
    resolve_for_read_open, resolve_for_write, tool_user_error_from_workspace_path,
};
use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::ToolContext;
use crate::cm_tools::tools::write_sse_preview::{
    WORKSPACE_WRITE_DIFF_BUDGET_CHARS, WriteDiffFileState,
    format_tool_output_with_write_diff_preview,
};
use crate::cm_tools::workspace::changelist::record_file_state_after_write;
use crate::cm_tools::workspace::path::WorkspacePathError;
use crate::cm_tools::workspace::fs::{
    copy_opened_file_under_root, rename_file_under_root, write_bytes_under_root,
};

use super::write_precheck::NOT_WRITTEN_HINT;

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

/// 路径解析失败：正文保持 `错误：…`，错误码以 `_failed` 结尾归入 `external`（与 Legacy 正文推断一致）。
fn workspace_path_tool_error(e: WorkspacePathError) -> ToolError {
    ToolError::external_code(
        "workspace_resolve_failed",
        tool_user_error_from_workspace_path(e),
    )
}

/// 写盘前校验失败：沿用 `校验失败（CODE）` 正文，并按该正文保留 `write_precheck_failed` 错误码。
fn write_precheck_tool_error(tool: &str, message: String) -> ToolError {
    let parsed = crate::cm_tools::tool_result::parse_legacy_output(tool, &message);
    ToolError::from_parsed_legacy(tool, &parsed, message)
}

/// 创建文件：仅在文件不存在时创建；若已存在则报错。
/// 参数 args_json: { "path": string, "content": string }
pub fn create_file(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    create_file_try(args_json, working_dir, ctx).unwrap_or_else(|e| e.message)
}

/// [`create_file`] 的显式失败版本（成功正文与 String 面逐字节一致）。
#[allow(clippy::result_large_err)]
pub fn create_file_try(
    args_json: &str,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    let (path, content) = parse_path_content(args_json).map_err(ToolError::invalid_args)?;
    let base = canonical_workspace_root(working_dir).map_err(workspace_path_tool_error)?;
    let target = resolve_for_write(working_dir, &path).map_err(workspace_path_tool_error)?;
    if target.exists() {
        return Err(ToolError::invalid_args(
            "错误：文件已存在，无法仅创建".to_string(),
        ));
    }
    let skip_precheck = super::write_precheck::arg_skip_precheck(args_json);
    if let Err(e) = super::write_precheck::precheck_before_write(&path, &content, skip_precheck) {
        return Err(write_precheck_tool_error("create_file", e));
    }
    write_bytes_under_root(&base, &target, content.as_bytes(), true, false)
        .map(|()| {
            record_file_state_after_write(ctx.workspace_changelist, working_dir, &path, None);
            let disp = path_for_tool_display(working_dir, &target, Some(&path));
            let body = tool_output_prepend_path(&disp, format!("已创建文件: {}", disp));
            format_tool_output_with_write_diff_preview(
                "create_file",
                true,
                body,
                vec![WriteDiffFileState {
                    rel_path: path.clone(),
                    before: None,
                    after: Some(content),
                }],
                WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
            )
        })
        .map_err(|e| ToolError::external_code("create_file_failed", format!("写入文件失败: {}", e)))
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
    copy_file_try(args_json, working_dir, ctx).unwrap_or_else(|e| e.message)
}

/// [`copy_file`] 的显式失败版本（成功正文与 String 面逐字节一致）。
#[allow(clippy::result_large_err)]
pub fn copy_file_try(
    args_json: &str,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    let (from, to, overwrite) =
        parse_from_to_overwrite(args_json).map_err(ToolError::invalid_args)?;
    let base = canonical_workspace_root(working_dir).map_err(workspace_path_tool_error)?;
    let opened = resolve_for_read_open(working_dir, &from).map_err(workspace_path_tool_error)?;
    if !opened.file.metadata().map(|m| m.is_file()).unwrap_or(false) {
        return Err(ToolError::invalid_args(
            "错误：源路径不是常规文件（或为目录），仅支持复制文件".to_string(),
        ));
    }
    let dst = resolve_for_write(working_dir, &to).map_err(workspace_path_tool_error)?;
    if opened.resolved_path == dst {
        return Err(ToolError::invalid_args(
            "错误：源与目标解析后相同，无需复制".to_string(),
        ));
    }
    if let Err(e) = check_dest_for_write_file(&dst, overwrite) {
        return Err(ToolError::invalid_args(e));
    }
    match copy_opened_file_under_root(&base, &opened, &dst, overwrite) {
        Ok(n) => {
            record_file_state_after_write(ctx.workspace_changelist, working_dir, &to, None);
            let body = tool_output_prepend_from_to(&from, &to, format!("已复制（{} 字节）", n));
            let after = std::fs::read_to_string(&dst).ok();
            Ok(format_tool_output_with_write_diff_preview(
                "copy_file",
                true,
                body,
                vec![WriteDiffFileState {
                    rel_path: to.clone(),
                    before: None,
                    after,
                }],
                WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
            ))
        }
        Err(e) => Err(ToolError::external_code(
            "copy_file_failed",
            format!("复制失败: {}", e),
        )),
    }
}

/// 在工作区内移动**文件**（重命名或迁路径）。`rename` 失败且为跨设备时自动回退为复制后删除源文件。
/// `overwrite` 默认 `false`：目标已存在为文件时须 `true` 才覆盖（与 `copy_file` 一致）。
pub fn move_file(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    move_file_try(args_json, working_dir, ctx).unwrap_or_else(|e| e.message)
}

/// [`move_file`] 的显式失败版本（成功正文与 String 面逐字节一致）。
#[allow(clippy::result_large_err)]
pub fn move_file_try(
    args_json: &str,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    let (from, to, overwrite) =
        parse_from_to_overwrite(args_json).map_err(ToolError::invalid_args)?;
    let base = canonical_workspace_root(working_dir).map_err(workspace_path_tool_error)?;
    let src = resolve_for_read(working_dir, &from).map_err(workspace_path_tool_error)?;
    if !src.is_file() {
        return Err(ToolError::invalid_args(
            "错误：源路径不是常规文件（或为目录），仅支持移动文件".to_string(),
        ));
    }
    let dst = resolve_for_write(working_dir, &to).map_err(workspace_path_tool_error)?;
    if src == dst {
        return Err(ToolError::invalid_args(
            "错误：源与目标解析后相同".to_string(),
        ));
    }
    if let Err(e) = check_dest_for_write_file(&dst, overwrite) {
        return Err(ToolError::invalid_args(e));
    }
    match rename_file_under_root(&base, &src, &dst) {
        Ok(()) => {
            record_file_state_after_write(ctx.workspace_changelist, working_dir, &from, None);
            record_file_state_after_write(ctx.workspace_changelist, working_dir, &to, None);
            Ok(tool_output_prepend_from_to(&from, &to, "已移动"))
        }
        Err(e) => Err(ToolError::external_code(
            "move_file_failed",
            format!("移动失败: {}", e),
        )),
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

#[allow(clippy::too_many_arguments)] // 整文件覆盖：路径、工作区、上下文、内容与三个写盘闸门标志
fn modify_file_write_full_overwrite(
    path: &str,
    target: &Path,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
    v: &serde_json::Value,
    content: String,
    dry_run: bool,
    confirm_full_overwrite: bool,
    skip_precheck: bool,
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
            true,
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
            format!(
                "错误：本次整文件覆盖将大幅缩短、删去大量行或清空非空文件。{NOT_WRITTEN_HINT}。\
下方为本次写入的 diff 预览；确认后使用 confirm_full_overwrite=true 再次调用，或改用 mode=replace_lines / mode=insert_after_line / search_replace 做局部修改。"
            ),
        );
        return format_tool_output_with_write_diff_preview(
            "modify_file",
            false,
            body,
            vec![WriteDiffFileState {
                rel_path: path.to_string(),
                before: before.clone(),
                after: Some(content.clone()),
            }],
            WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
        );
    }

    if let Err(e) = super::write_precheck::precheck_before_write(path, &content, skip_precheck) {
        return e;
    }
    // 统一提交入口：写入临时文件后、rename 前一刻再做 revision 校验（显式 base_revision
    // 始终校验；full 模式不做轮次自动绑定，因为整文件覆盖不依赖行号快照）。
    if let Err(e) = super::commit_guard::write_bytes_with_revision_guard(
        target,
        content.as_bytes(),
        v,
        ctx,
        false,
    ) {
        return format!("写入文件失败: {}", e);
    }

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
        true,
        body,
        vec![WriteDiffFileState {
            rel_path: path.to_string(),
            before: before_preview,
            after,
        }],
        WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    )
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

/// 行号类模式依赖“当前文件坐标”，默认使用本轮 `read_file` 的 revision 自动校验；
/// 整文件覆盖不依赖行号，不做自动绑定。
fn modify_file_auto_bind_revision(v: &serde_json::Value, mode: &str) -> bool {
    v.get("edits").is_some()
        || matches!(
            mode,
            "replace_lines" | "lines" | "replacelines" | "insert_after_line"
        )
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
    if v.get("expect_content").is_some() || v.get("expect_line_content").is_some() {
        return "错误：expect_content/expect_line_content 是按行局部编辑的守卫参数，不能与 mode=full/overwrite 混用；请改用 mode=replace_lines / insert_after_line。"
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
    let skip_precheck = v
        .get("skip_precheck")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    modify_file_write_full_overwrite(
        path,
        target,
        working_dir,
        ctx,
        v,
        content,
        dry_run,
        confirm_full_overwrite,
        skip_precheck,
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
    // 批量 edits 优先于 mode 推断：edits 与顶层 mode/行号互斥，冲突信息由批量入口给出。
    if v.get("edits").is_some() {
        let display = modify_file_display(working_dir, target, path);
        return super::edits_batch::modify_file_batch_edits(
            v, target, working_dir, ctx, path, &display,
        );
    }
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
    let auto_bind = modify_file_auto_bind_revision(&v, &mode);
    if let Err(e) = super::revision::verify_write_revision(&v, &target, ctx, auto_bind) {
        return e;
    }

    modify_file_dispatch_mode(&v, &target, working_dir, ctx, &path, explicit_mode, &mode)
}
