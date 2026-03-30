//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use std::path::Path;

use super::path::{path_for_tool_display, resolve_for_read, tool_user_error_from_workspace_path};

// ── chmod_file ──────────────────────────────────────────────

#[cfg(unix)]
pub fn chmod_file(args_json: &str, working_dir: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;

    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v.get("path").and_then(|p| p.as_str()).map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "缺少 path 参数".to_string(),
    };
    let mode_str = match v.get("mode").and_then(|m| m.as_str()).map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "缺少 mode 参数（如 \"755\"、\"644\"）".to_string(),
    };
    let confirm = v.get("confirm").and_then(|c| c.as_bool()).unwrap_or(false);
    if !confirm {
        return "拒绝执行：chmod_file 需要 confirm=true".to_string();
    }

    let mode = match u32::from_str_radix(&mode_str, 8) {
        Ok(m) if m <= 0o7777 => m,
        _ => {
            return format!(
                "错误：mode \"{}\" 不是合法的八进制权限值（如 755、644）",
                mode_str
            );
        }
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
