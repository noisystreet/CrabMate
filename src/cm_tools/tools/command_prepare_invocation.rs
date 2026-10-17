//! `run_command` 调用准备：解析 argv、白名单/工作区可执行、参数安全。
//! 作为 [`super`]（`command`）的子模块，经 `#[path]` 挂载。

use std::path::{Path, PathBuf};

use super::super::command_line_prepare::{
    arg_has_parent_dir_ref, is_arg_safe, merge_dot_slash_with_single_relative_path,
    parse_cd_prefix, peel_workspace_cd_prefix, split_command_prefix_if_embedded,
};
use super::{
    PreparedRunCommand, RunCommandError, check_shell_variable_references,
    command_shell_script::{
        allowlist_allows_all, argv_needs_posix_shell_wrap, is_shell_dash_c_invocation,
        maybe_wrap_argv_with_posix_shell, posix_shell_on_allowlist,
    },
};

/// 规范化后的 `run_command` 调用参数（纯解析；不含工作目录/白名单裁定）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// 原始 args JSON 文本（`workspace_image_chat_hint` 等消费方需要原文）。
    pub args_json: String,
    /// 已 trim，并已做 `./` 合并与整段命令拆分的命令名。
    pub command: String,
    pub args: Vec<String>,
}

/// 解析 `run_command` 参数：JSON 修复 → 字段提取 → 规范化。
///
/// `working_dir` 仅用于把工作区内绝对路径改写成相对路径；不做文件系统存在性校验，也不做白名单裁定。
pub fn parse_command_spec(
    args_json: &str,
    working_dir: &Path,
) -> Result<CommandSpec, RunCommandError> {
    let value = super::parse_run_command_value_with_repair(args_json)?;
    let (command, args) = extract_run_command_name_and_args(&value, working_dir)?;
    Ok(CommandSpec {
        args_json: args_json.to_string(),
        command,
        args,
    })
}

/// 宽松解析：供去重 / 预算守卫 / 只读缓存等只读消费方。
///
/// 不走 JSON 修复链、不做路径规范化；`args` 非数组时视为空，缺失/非字符串 `command` 返回 `None`。
/// 空 `command` 是否接受由调用方决定。
pub fn parse_run_command_payload_lenient(args_json: &str) -> Option<(String, Vec<String>)> {
    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
    let command = v.get("command")?.as_str()?.trim().to_string();
    let args = v
        .get("args")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some((command, args))
}

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
        && !allowlist_allows_all(allowed_commands)
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

fn scan_cd_join_escape(
    candidate: &Path,
    anchor: &Path,
    pattern_unsafe: bool,
    effective_working_dir: &mut PathBuf,
) -> Result<bool, RunCommandError> {
    if !candidate.is_dir() {
        return Ok(false);
    }
    match candidate.canonicalize() {
        Ok(canon_cand) => {
            if !canon_cand.starts_with(anchor) {
                Ok(true)
            } else {
                if !pattern_unsafe {
                    *effective_working_dir = canon_cand;
                }
                Ok(false)
            }
        }
        Err(e) => Err(RunCommandError::SpawnOther {
            cmd: format!("canonicalize({})", candidate.display()),
            source: e,
        }),
    }
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
        let Some((dir, rest)) = parse_cd_prefix(cmd_args) else {
            // 与 peel 一致：不完整前缀交给后续 peel/prepare 报 CdPrefixInvalid
            break;
        };
        let dir = dir.to_string();
        let mut rest = rest.to_vec();
        let pattern_unsafe = !is_arg_safe("cd", &dir);
        let candidate = effective_working_dir.join(&dir);
        let escape_unsafe =
            scan_cd_join_escape(&candidate, &anchor, pattern_unsafe, effective_working_dir)?;
        if (pattern_unsafe || escape_unsafe) && !unsafe_out.iter().any(|u| u == &dir) {
            unsafe_out.push(dir);
        }
        *cmd_raw = rest.remove(0);
        *cmd_args = rest;
        split_command_prefix_if_embedded(cmd_raw, cmd_args);
    }
    Ok(())
}

/// 与 [`prepare_run_command_spec`] 同源解析后，返回按 validate 规则判定为不安全的参数。
/// 空 `Vec` = 无需外部路径审批。`cmake` 的 `..` 不入列（与 [`is_arg_safe`] 一致）。
pub fn scan_run_command_unsafe_args_spec(
    spec: &CommandSpec,
    working_dir: &Path,
    allowed_commands: &[String],
) -> Result<Vec<String>, RunCommandError> {
    let mut cmd_raw = spec.command.clone();
    let mut cmd_args = spec.args.clone();
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

/// 与 [`scan_run_command_unsafe_args_spec`] 同源：从原始 args JSON 解析后扫描
/// argv 级不安全参数（空 = 无需外部路径审批）。
pub fn scan_run_command_unsafe_args_json(
    args_json: &str,
    working_dir: &Path,
    allowed_commands: &[String],
) -> Result<Vec<String>, RunCommandError> {
    let spec = parse_command_spec(args_json, working_dir)?;
    scan_run_command_unsafe_args_spec(&spec, working_dir, allowed_commands)
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

pub(super) fn prepare_run_command_spec(
    spec: &CommandSpec,
    working_dir: &Path,
    allowed_commands: &[String],
    skip_arg_safety: bool,
) -> Result<PreparedRunCommand, RunCommandError> {
    let mut cmd_raw = spec.command.clone();
    let mut cmd_args = spec.args.clone();

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

    use super::super::RunCommandError;
    use super::{
        parse_command_spec, prepare_run_command_spec, scan_run_command_unsafe_args_spec,
    };

    fn test_allowed() -> Vec<String> {
        vec![
            "cat".into(),
            "cmake".into(),
            "git".into(),
            "cd".into(),
            "ls".into(),
        ]
    }

    fn spec(args_json: &str) -> super::CommandSpec {
        parse_command_spec(args_json, Path::new(".")).expect("spec")
    }

    #[test]
    fn scan_lists_external_absolute_path_not_cmake_dotdot() {
        let u = scan_run_command_unsafe_args_spec(
            &spec(r#"{"command":"cat","args":["/etc/passwd"]}"#),
            Path::new("."),
            &test_allowed(),
        )
        .expect("scan");
        assert_eq!(u, vec!["/etc/passwd".to_string()]);

        let u = scan_run_command_unsafe_args_spec(
            &spec(r#"{"command":"cmake","args":[".."]}"#),
            Path::new("."),
            &test_allowed(),
        )
        .expect("scan");
        assert!(u.is_empty(), "{u:?}");

        let u = scan_run_command_unsafe_args_spec(
            &spec(r#"{"command":"git","args":["log","main..HEAD"]}"#),
            Path::new("."),
            &test_allowed(),
        )
        .expect("scan");
        assert!(
            u.is_empty(),
            "git A..B must not look like path traversal: {u:?}"
        );
    }

    #[test]
    fn scan_lists_cd_parent_dir_prefix() {
        let u = scan_run_command_unsafe_args_spec(
            &spec(r#"{"command":"cd","args":["..","&&","ls"]}"#),
            Path::new("."),
            &test_allowed(),
        )
        .expect("scan");
        assert!(u.iter().any(|a| a == ".."), "{u:?}");
    }

    #[test]
    fn star_sentinel_allows_arbitrary_command_name() {
        let args_json = r#"{"command":"whoami","args":[]}"#;
        let spec = parse_command_spec(args_json, Path::new(".")).expect("spec");
        let star = vec!["*".to_string()];
        let prepared = prepare_run_command_spec(&spec, Path::new("."), &star, false).expect("star");
        assert_eq!(prepared.cmd_name, "whoami");

        let denied = prepare_run_command_spec(&spec, Path::new("."), &test_allowed(), false);
        assert!(matches!(
            denied,
            Err(RunCommandError::DisallowedCommand { .. })
        ));
    }
}
