//! `run_command` 调用准备：解析 argv、白名单/工作区可执行、参数安全。
//! 作为 [`super`]（`command`）的子模块，经 `#[path]` 挂载。

use std::path::{Path, PathBuf};

use super::super::command_line_prepare::{
    is_arg_safe, merge_dot_slash_with_single_relative_path, peel_workspace_cd_prefix,
    split_command_prefix_if_embedded,
};
use super::{PreparedRunCommand, RunCommandError, check_shell_variable_references};

fn normalize_workspace_absolute_arg(arg: &str, working_dir: &Path) -> String {
    let a = arg.trim();
    if !a.starts_with('/') {
        return arg.to_string();
    }
    let Ok(rel) = Path::new(a).strip_prefix(working_dir) else {
        return arg.to_string();
    };
    if rel.as_os_str().is_empty() {
        ".".to_string()
    } else {
        rel.to_string_lossy().to_string()
    }
}

/// 从 JSON 取出 `command`/`args`，并做绝对路径归一化与常见误拆修复。
fn extract_run_command_name_and_args(
    args: &serde_json::Value,
    working_dir: &Path,
) -> Result<(String, Vec<String>), RunCommandError> {
    let mut cmd_raw = match args.get("command").and_then(|c| c.as_str()) {
        Some(s) => s.trim().to_string(),
        None => return Err(RunCommandError::MissingCommand),
    };

    let mut cmd_args: Vec<String> = match args.get("args") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(_) => return Err(RunCommandError::ArgsNotArray),
        None => vec![],
    };
    cmd_args = cmd_args
        .into_iter()
        .map(|a| normalize_workspace_absolute_arg(&a, working_dir))
        .collect();

    merge_dot_slash_with_single_relative_path(&mut cmd_raw, &mut cmd_args);
    split_command_prefix_if_embedded(&mut cmd_raw, &mut cmd_args);
    Ok((cmd_raw, cmd_args))
}

/// 解析工作区可执行路径；否则要求命令名在白名单内。
fn resolve_run_command_exec_path(
    cmd_raw: &str,
    effective_working_dir: &Path,
    allowed_commands: &[String],
) -> Result<(String, Option<PathBuf>), RunCommandError> {
    let cmd_name = cmd_raw.to_lowercase();
    let is_workspace_executable = cmd_raw.starts_with("./") || cmd_raw.contains('/');
    let exec_path = if is_workspace_executable {
        crate::tools::resolve_workspace_executable(effective_working_dir, cmd_raw).ok()
    } else {
        None
    };

    if exec_path.is_none()
        && !allowed_commands
            .iter()
            .any(|c| c.eq_ignore_ascii_case(&cmd_name))
    {
        return Err(RunCommandError::DisallowedCommand {
            attempted: cmd_name,
            allowed: allowed_commands.join(", "),
        });
    }
    Ok((cmd_name, exec_path))
}

fn validate_run_command_args_safety(
    cmd_name: &str,
    cmd_args: &[String],
    has_workspace_exec: bool,
) -> Result<(), RunCommandError> {
    if has_workspace_exec {
        for a in cmd_args {
            if a.contains("..") || a.trim_start().starts_with('/') {
                return Err(RunCommandError::UnsafeArg);
            }
        }
    } else {
        for a in cmd_args {
            if !is_arg_safe(cmd_name, a) {
                return Err(RunCommandError::UnsafeArg);
            }
        }
    }
    Ok(())
}

pub(super) fn prepare_run_command_invocation(
    args: &serde_json::Value,
    working_dir: &Path,
    allowed_commands: &[String],
) -> Result<PreparedRunCommand, RunCommandError> {
    let (mut cmd_raw, mut cmd_args) = extract_run_command_name_and_args(args, working_dir)?;

    let mut effective_working_dir = working_dir.to_path_buf();
    peel_workspace_cd_prefix(
        working_dir,
        &mut effective_working_dir,
        &mut cmd_raw,
        &mut cmd_args,
    )?;

    check_shell_variable_references(&cmd_raw, &cmd_args)?;

    let (cmd_name, exec_path) =
        resolve_run_command_exec_path(&cmd_raw, &effective_working_dir, allowed_commands)?;
    validate_run_command_args_safety(&cmd_name, &cmd_args, exec_path.is_some())?;

    Ok(PreparedRunCommand {
        cmd_raw,
        cmd_name,
        exec_path,
        cmd_args,
        effective_working_dir,
    })
}
