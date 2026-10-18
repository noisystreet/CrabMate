//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use std::path::Path;

use super::path::{path_for_tool_display, resolve_for_read};

use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::tool_param_types::ChmodFileArgs;

// ── chmod_file ──────────────────────────────────────────────

#[cfg(unix)]
fn parse_unix_mode_octal(mode_str: &str) -> Result<u32, String> {
    match u32::from_str_radix(mode_str, 8) {
        Ok(m) if m <= 0o7777 => Ok(m),
        _ => Err(format!(
            "错误：mode \"{}\" 不是合法的八进制权限值（如 755、644）",
            mode_str
        )),
    }
}

#[cfg(unix)]
fn parse_chmod_args(args_json: &str) -> Result<(String, String, u32), String> {
    let args: ChmodFileArgs = crate::cm_tools::tools::parse_args_typed(args_json)?;
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return Err("缺少 path 参数".to_string()),
    };
    let mode_str = match args.mode.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return Err("缺少 mode 参数（如 \"755\"、\"644\"）".to_string()),
    };
    if !args.confirm.unwrap_or(false) {
        return Err("拒绝执行：chmod_file 需要 confirm=true".to_string());
    }
    let mode = parse_unix_mode_octal(&mode_str)?;
    Ok((path, mode_str, mode))
}

#[cfg(unix)]
pub fn chmod_file(args_json: &str, working_dir: &Path) -> String {
    chmod_file_try(args_json, working_dir).unwrap_or_else(|e| e.message)
}

/// [`chmod_file`] 的显式失败版本（成功正文与 String 面逐字节一致）。
#[cfg(unix)]
#[allow(clippy::result_large_err)]
pub fn chmod_file_try(args_json: &str, working_dir: &Path) -> Result<String, ToolError> {
    use std::os::unix::fs::PermissionsExt;

    let (path, mode_str, mode) = parse_chmod_args(args_json).map_err(ToolError::invalid_args)?;

    let target = resolve_for_read(working_dir, &path).map_err(ToolError::path_resolve_failed)?;

    let perms = std::fs::Permissions::from_mode(mode);
    match std::fs::set_permissions(&target, perms) {
        Ok(()) => Ok(format!(
            "已设置权限 {} → {}",
            path_for_tool_display(working_dir, &target, Some(&path)),
            mode_str
        )),
        Err(e) => Err(ToolError::external_code(
            "chmod_file_failed",
            format!("设置权限失败：{}", e),
        )),
    }
}

#[cfg(not(unix))]
pub fn chmod_file(args_json: &str, working_dir: &Path) -> String {
    chmod_file_try(args_json, working_dir).unwrap_or_else(|e| e.message)
}

/// [`chmod_file`] 的显式失败版本（非 Unix 平台恒失败）。
#[cfg(not(unix))]
#[allow(clippy::result_large_err)]
pub fn chmod_file_try(_args_json: &str, _working_dir: &Path) -> Result<String, ToolError> {
    Err(ToolError::external_code(
        "chmod_file_failed",
        "错误：chmod_file 仅在 Unix/Linux 系统上可用",
    ))
}
