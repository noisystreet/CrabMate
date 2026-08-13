//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use std::path::Path;

use super::path::{path_for_tool_display, resolve_for_read, tool_user_error_from_workspace_path};

use crate::tools::tool_param_types::ChmodFileArgs;

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
    let v = crate::tools::parse_args_json(args_json)?;
    let args: ChmodFileArgs =
        serde_json::from_value(v).map_err(|e| format!("参数解析错误: {e}"))?;
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
    use std::os::unix::fs::PermissionsExt;

    let (path, mode_str, mode) = match parse_chmod_args(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };

    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };

    let perms = std::fs::Permissions::from_mode(mode);
    match std::fs::set_permissions(&target, perms) {
        Ok(()) => format!(
            "已设置权限 {} → {}",
            path_for_tool_display(working_dir, &target, Some(&path)),
            mode_str
        ),
        Err(e) => format!("设置权限失败：{}", e),
    }
}

#[cfg(not(unix))]
pub fn chmod_file(_args_json: &str, _working_dir: &Path) -> String {
    "错误：chmod_file 仅在 Unix/Linux 系统上可用".to_string()
}
