//! `run_command` 调用准备：解析 argv、白名单/工作区可执行、参数安全。
//! 作为 [`super`]（`command`）的子模块，经 `#[path]` 挂载。

use std::path::{Path, PathBuf};

use super::super::command_line_prepare::{
    arg_has_parent_dir_ref, is_arg_safe, merge_dot_slash_with_single_relative_path,
    peel_workspace_cd_prefix, split_command_prefix_if_embedded,
};
use super::{
    PreparedRunCommand, RunCommandError, check_shell_variable_references,
    command_shell_script::{
        argv_needs_posix_shell_wrap, is_shell_dash_c_invocation, maybe_wrap_argv_with_posix_shell,
        posix_shell_on_allowlist,
    },
};

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
        crate::cm_tools::tools::resolve_workspace_executable(effective_working_dir, cmd_raw).ok()
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

fn arg_is_unsafe_for_cmd(cmd_name: &str, arg: &str, has_workspace_exec: bool) -> bool {
    if has_workspace_exec {
        arg_has_parent_dir_ref(arg) || arg.trim_start().starts_with('/')
    } else {
        !is_arg_safe(cmd_name, arg)
    }
}

fn validate_run_command_args_safety(
    cmd_name: &str,
    cmd_args: &[String],
    has_workspace_exec: bool,
) -> Result<(), RunCommandError> {
    for a in cmd_args {
        if arg_is_unsafe_for_cmd(cmd_name, a, has_workspace_exec) {
            return Err(RunCommandError::UnsafeArg);
        }
    }
    Ok(())
}

fn collect_unsafe_cmd_args(
    cmd_name: &str,
    cmd_args: &[String],
    has_workspace_exec: bool,
) -> Vec<String> {
    cmd_args
        .iter()
        .filter(|a| arg_is_unsafe_for_cmd(cmd_name, a, has_workspace_exec))
        .cloned()
        .collect()
}

/// 扫描用：推进 `cd … &&` 前缀；不安全的 `cd` 目录记入 `unsafe_out` 后仍合成推进以便继续扫后续 argv。
fn advance_cd_prefixes_collecting_unsafe(
    workspace_root: &Path,
    effective_working_dir: &mut PathBuf,
    cmd_raw: &mut String,
    cmd_args: &mut Vec<String>,
    unsafe_out: &mut Vec<String>,
) -> Result<(), RunCommandError> {
    let anchor = workspace_root
        .canonicalize()
        .map_err(|e| RunCommandError::SpawnOther {
            cmd: "canonicalize(workspace)".to_string(),
            source: e,
        })?;
    loop {
        if !cmd_raw.eq_ignore_ascii_case("cd") {
            break;
        }
        if cmd_args.len() < 3 || cmd_args[1] != "&&" {
            // 与 peel 一致：交给后续 peel/prepare 报 CdPrefixInvalid
            break;
        }
        let dir = cmd_args[0].trim().to_string();
        let pattern_unsafe = !is_arg_safe("cd", &dir);
        let candidate = effective_working_dir.join(&dir);
        let mut escape_unsafe = false;
        if candidate.is_dir() {
            match candidate.canonicalize() {
                Ok(canon_cand) => {
                    if !canon_cand.starts_with(&anchor) {
                        escape_unsafe = true;
                    } else if !pattern_unsafe {
                        *effective_working_dir = canon_cand;
                    }
                }
                Err(e) => {
                    return Err(RunCommandError::SpawnOther {
                        cmd: format!("canonicalize({})", candidate.display()),
                        source: e,
                    });
                }
            }
        }
        if (pattern_unsafe || escape_unsafe) && !unsafe_out.iter().any(|u| u == &dir) {
            unsafe_out.push(dir);
        }
        *cmd_args = cmd_args[2..].to_vec();
        if cmd_args.is_empty() {
            return Err(RunCommandError::MissingCommand);
        }
        *cmd_raw = cmd_args[0].clone();
        let rest: Vec<String> = cmd_args[1..].to_vec();
        *cmd_args = rest;
        split_command_prefix_if_embedded(cmd_raw, cmd_args);
    }
    Ok(())
}

/// 与 [`prepare_run_command_invocation`] 同源解析后，返回按 validate 规则判定为不安全的参数。
/// 空 `Vec` = 无需外部路径审批。`cmake` 的 `..` 不入列（与 [`is_arg_safe`] 一致）。
pub fn scan_run_command_unsafe_args(
    args: &serde_json::Value,
    working_dir: &Path,
    allowed_commands: &[String],
) -> Result<Vec<String>, RunCommandError> {
    let (mut cmd_raw, mut cmd_args) = extract_run_command_name_and_args(args, working_dir)?;
    let mut effective_working_dir = working_dir.to_path_buf();
    let mut unsafe_args = Vec::new();
    advance_cd_prefixes_collecting_unsafe(
        working_dir,
        &mut effective_working_dir,
        &mut cmd_raw,
        &mut cmd_args,
        &mut unsafe_args,
    )?;
    // 扫描阶段：命令未在白名单时仍收集 argv 危险参数（调用方通常已先做命令名审批）。
    // 不在此拦截 glob/`$VAR`：展开改由 bash -c 或异步审批处理。
    let cmd_name = cmd_raw.to_lowercase();
    let is_workspace_executable = cmd_raw.starts_with("./") || cmd_raw.contains('/');
    let has_workspace_exec = if is_workspace_executable {
        crate::cm_tools::tools::resolve_workspace_executable(effective_working_dir.as_path(), &cmd_raw)
            .is_ok()
    } else {
        false
    };
    let (cmd_name, has_exec) =
        match resolve_run_command_exec_path(&cmd_raw, &effective_working_dir, allowed_commands) {
            Ok((name, exec)) => (name, exec.is_some()),
            Err(RunCommandError::DisallowedCommand { .. }) => (cmd_name, has_workspace_exec),
            Err(e) => return Err(e),
        };
    let more = collect_unsafe_cmd_args(&cmd_name, &cmd_args, has_exec);
    for a in more {
        if !unsafe_args.iter().any(|u| u == &a) {
            unsafe_args.push(a);
        }
    }
    Ok(unsafe_args)
}

fn skip_shell_variable_check(cmd_raw: &str, cmd_args: &[String], allowed: &[String]) -> bool {
    is_shell_dash_c_invocation(cmd_raw, cmd_args)
        || (posix_shell_on_allowlist(allowed).is_some()
            && argv_needs_posix_shell_wrap(cmd_raw, cmd_args))
}

fn wrap_posix_shell_reresolve(
    cmd_raw: &mut String,
    cmd_args: &mut Vec<String>,
    cmd_name: &mut String,
    exec_path: &mut Option<PathBuf>,
    working_dir: &Path,
    allowed_commands: &[String],
) -> Result<(), RunCommandError> {
    if maybe_wrap_argv_with_posix_shell(cmd_raw, cmd_args, allowed_commands) {
        let wrapped = resolve_run_command_exec_path(cmd_raw, working_dir, allowed_commands)?;
        *cmd_name = wrapped.0;
        *exec_path = wrapped.1;
    }
    Ok(())
}

pub(super) fn prepare_run_command_invocation(
    args: &serde_json::Value,
    working_dir: &Path,
    allowed_commands: &[String],
    skip_arg_safety: bool,
) -> Result<PreparedRunCommand, RunCommandError> {
    let (mut cmd_raw, mut cmd_args) = extract_run_command_name_and_args(args, working_dir)?;

    let mut effective_working_dir = working_dir.to_path_buf();
    peel_workspace_cd_prefix(
        working_dir,
        &mut effective_working_dir,
        &mut cmd_raw,
        &mut cmd_args,
        skip_arg_safety,
    )?;

    let inject_gh_token = crate::cm_tools::github_token::command_basename_is_gh(&cmd_raw);
    if !skip_shell_variable_check(&cmd_raw, &cmd_args, allowed_commands) {
        check_shell_variable_references(&cmd_raw, &cmd_args)?;
    }

    let (mut cmd_name, mut exec_path) =
        resolve_run_command_exec_path(&cmd_raw, &effective_working_dir, allowed_commands)?;
    if !skip_arg_safety {
        validate_run_command_args_safety(&cmd_name, &cmd_args, exec_path.is_some())?;
    }
    wrap_posix_shell_reresolve(
        &mut cmd_raw,
        &mut cmd_args,
        &mut cmd_name,
        &mut exec_path,
        &effective_working_dir,
        allowed_commands,
    )?;

    Ok(PreparedRunCommand {
        cmd_raw,
        cmd_name,
        exec_path,
        cmd_args,
        effective_working_dir,
        inject_gh_token,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::scan_run_command_unsafe_args;

    fn test_allowed() -> Vec<String> {
        vec![
            "cat".into(),
            "cmake".into(),
            "git".into(),
            "cd".into(),
            "ls".into(),
        ]
    }

    #[test]
    fn scan_lists_external_absolute_path_not_cmake_dotdot() {
        let abs: serde_json::Value =
            serde_json::from_str(r#"{"command":"cat","args":["/etc/passwd"]}"#).expect("json");
        let u = scan_run_command_unsafe_args(&abs, Path::new("."), &test_allowed()).expect("scan");
        assert_eq!(u, vec!["/etc/passwd".to_string()]);

        let cmake: serde_json::Value =
            serde_json::from_str(r#"{"command":"cmake","args":[".."]}"#).expect("json");
        let u =
            scan_run_command_unsafe_args(&cmake, Path::new("."), &test_allowed()).expect("scan");
        assert!(u.is_empty(), "{u:?}");

        let git_range: serde_json::Value =
            serde_json::from_str(r#"{"command":"git","args":["log","main..HEAD"]}"#).expect("json");
        let u = scan_run_command_unsafe_args(&git_range, Path::new("."), &test_allowed())
            .expect("scan");
        assert!(
            u.is_empty(),
            "git A..B must not look like path traversal: {u:?}"
        );
    }

    #[test]
    fn scan_lists_cd_parent_dir_prefix() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"command":"cd","args":["..","&&","ls"]}"#).expect("json");
        let u = scan_run_command_unsafe_args(&v, Path::new("."), &test_allowed()).expect("scan");
        assert!(u.iter().any(|a| a == ".."), "{u:?}");
    }
}
