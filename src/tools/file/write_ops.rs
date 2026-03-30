//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use super::path::{
    parse_path_content, path_for_tool_display, resolve_for_read, resolve_for_write,
    tool_user_error_from_workspace_path,
};
use crate::tools::ToolContext;
use crate::workspace_changelist::record_file_state_after_write;

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
            format!(
                "已创建文件: {}",
                path_for_tool_display(working_dir, &target, Some(&path))
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
    let v: serde_json::Value =
        serde_json::from_str(args_json).map_err(|e| format!("参数 JSON 无效: {}", e))?;
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
            format!("已复制：{} -> {}（{} 字节）", from, to, n)
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
            format!("已移动：{} -> {}", from, to)
        }
        Err(e) => format!("移动失败: {}", e),
    }
}

/// 修改文件：仅在文件已存在时写入。
/// - 默认 `mode`=`full`：整文件覆盖（`content` 为全文）。
/// - `mode`=`replace_lines`：`start_line`..=`end_line`（1-based，含边界）替换为 `content`（流式读写，适合大文件）。
pub fn modify_file(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
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
        modify_file_replace_lines(&v, &target, &display, ctx, working_dir, &path)
    } else if mode == "full" || mode.is_empty() {
        let content = v
            .get("content")
            .and_then(|c| c.as_str())
            .map(String::from)
            .unwrap_or_default();
        let before = std::fs::read_to_string(&target).ok();
        match std::fs::write(&target, content.as_bytes()) {
            Ok(()) => {
                record_file_state_after_write(ctx.workspace_changelist, working_dir, &path, before);
                format!(
                    "已整文件覆盖: {}",
                    path_for_tool_display(working_dir, &target, Some(&path))
                )
            }
            Err(e) => format!("写入文件失败: {}", e),
        }
    } else {
        format!("错误：mode 仅支持 full 或 replace_lines（收到 {:?}）", mode)
    }
}

fn modify_file_replace_lines(
    v: &serde_json::Value,
    target: &Path,
    display_path: &str,
    ctx: &ToolContext<'_>,
    working_dir: &Path,
    rel_path: &str,
) -> String {
    let original = std::fs::read_to_string(target).ok();
    let start_line = match v.get("start_line").and_then(|n| n.as_u64()) {
        Some(n) if n >= 1 => n as usize,
        _ => return "错误：replace_lines 需要 start_line（>=1）".to_string(),
    };
    let end_line = match v.get("end_line").and_then(|n| n.as_u64()) {
        Some(n) if n >= 1 => n as usize,
        _ => return "错误：replace_lines 需要 end_line（>=1）".to_string(),
    };
    if end_line < start_line {
        return "错误：end_line 不能小于 start_line".to_string();
    }

    let new_body = v
        .get("content")
        .and_then(|c| c.as_str())
        .map(String::from)
        .unwrap_or_default();

    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return "错误：无法解析目标文件父目录".to_string(),
    };
    let fname = target
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("file");
    let tmp_path = parent.join(format!(".{fname}.crabmate_edit_tmp"));

    let src = match File::open(target) {
        Ok(f) => f,
        Err(e) => return format!("读取原文件失败: {}", e),
    };
    let tmp_file = match File::create(&tmp_path) {
        Ok(f) => f,
        Err(e) => return format!("创建临时文件失败: {}", e),
    };
    let mut reader = BufReader::new(src);
    let mut writer = BufWriter::new(tmp_file);
    let mut line_no: usize = 0;
    let mut replaced = false;
    let mut buf = String::new();

    loop {
        buf.clear();
        let n = match reader.read_line(&mut buf) {
            Ok(n) => n,
            Err(e) => return format!("读取原文件失败: {}", e),
        };
        if n == 0 {
            break;
        }
        line_no += 1;
        if line_no < start_line {
            if let Err(e) = writer.write_all(buf.as_bytes()) {
                return format!("写入临时文件失败: {}", e);
            }
            continue;
        }
        if line_no == start_line {
            if !new_body.is_empty() {
                if let Err(e) = writer.write_all(new_body.as_bytes()) {
                    return format!("写入临时文件失败: {}", e);
                }
                if !new_body.ends_with('\n')
                    && let Err(e) = writer.write_all(b"\n")
                {
                    return format!("写入临时文件失败: {}", e);
                }
            }
            replaced = true;
        }
        if line_no >= start_line && line_no <= end_line {
            continue;
        }
        if line_no > end_line
            && let Err(e) = writer.write_all(buf.as_bytes())
        {
            return format!("写入临时文件失败: {}", e);
        }
    }

    if line_no < start_line {
        return format!(
            "错误：start_line={} 超出文件行数（文件共 {} 行）",
            start_line, line_no
        );
    }
    if line_no < end_line {
        return format!(
            "错误：end_line={} 超出文件行数（文件共 {} 行）",
            end_line, line_no
        );
    }
    if !replaced {
        return "错误：未执行替换（内部状态异常）".to_string();
    }

    if let Err(e) = writer.flush() {
        let _ = std::fs::remove_file(&tmp_path);
        return format!("刷新临时文件失败: {}", e);
    }
    drop(writer);
    // Windows 上 rename 不能覆盖已存在目标，需先删原文件
    if target.exists()
        && let Err(e) = std::fs::remove_file(target)
    {
        let _ = std::fs::remove_file(&tmp_path);
        return format!("删除原文件以替换失败: {}", e);
    }
    if let Err(e) = std::fs::rename(&tmp_path, target) {
        let _ = std::fs::remove_file(&tmp_path);
        return format!("替换目标文件失败: {}", e);
    }

    record_file_state_after_write(ctx.workspace_changelist, working_dir, rel_path, original);
    format!(
        "已按行替换: {} (行 {}-{}，共删除 {} 行，写入新内容 {} 字节)",
        display_path,
        start_line,
        end_line,
        end_line - start_line + 1,
        new_body.len()
    )
}
