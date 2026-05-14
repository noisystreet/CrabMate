//! `run_command` 入参规范化：`command` 整段拆分、`cd <相对> && …` 前缀展开（无 shell）。

use std::io;
use std::path::{Path, PathBuf};

/// [`crate::tools::command::RunCommandError`] 在剥离 `cd` 前缀阶段的子集，避免 `command` ↔ 本模块循环依赖。
#[derive(Debug)]
pub(crate) enum CdPeelError {
    CdPrefixInvalid { detail: String, work_dir: String },
    UnsafeArg,
    MissingCommand,
    SpawnOther { cmd: String, source: io::Error },
}

pub(crate) fn is_arg_safe(cmd_name: &str, arg: &str) -> bool {
    let a = arg.trim();
    // cd 允许相对路径（禁止 .. 和绝对路径）
    if cmd_name == "cd" {
        return !a.contains("..") && !a.starts_with('/');
    }
    // cmake 允许 .. (用于 cmake .. 从 build 目录配置源目录)
    if cmd_name == "cmake" {
        return !a.starts_with('/');
    }
    // 其他命令禁止 .. 和绝对路径
    !a.contains("..") && !a.starts_with('/')
}

/// 将 `command` 写成 `prog arg1 arg2` 整段而 `args` 为空（或需前缀拼接）的常见误用，规范为
/// `prog` + `["arg1","arg2", …原 args…]`，以便 [`std::process::Command::new`] 能解析到真实可执行文件。
///
/// 含 `/` 的值视为路径（含 `./` 与 `subdir/tool`），不做拆分，避免误伤带空格的可执行路径。
pub(crate) fn split_command_prefix_if_embedded(cmd_raw: &mut String, cmd_args: &mut Vec<String>) {
    if cmd_raw.contains('/') {
        return;
    }
    let parts = cmd_mate::split_command_line(cmd_raw);
    if parts.len() <= 1 {
        return;
    }
    let head = parts[0].clone();
    if head.is_empty() {
        return;
    }
    let mut prefix: Vec<String> = parts[1..].to_vec();
    prefix.append(cmd_args);
    *cmd_args = prefix;
    *cmd_raw = head;
}

fn cd_prefix_invalid(work_dir: &Path, detail: impl Into<String>) -> CdPeelError {
    CdPeelError::CdPrefixInvalid {
        detail: detail.into(),
        work_dir: work_dir.display().to_string(),
    }
}

/// 将 `cd rel && …` 前缀展开为嵌套工作目录与真实 argv（无 shell；`rel` 不得含 `..`；目录必须已存在且落在 `workspace_root` 规范路径之下）。
pub(crate) fn peel_workspace_cd_prefix(
    workspace_root: &Path,
    effective_working_dir: &mut PathBuf,
    cmd_raw: &mut String,
    cmd_args: &mut Vec<String>,
) -> Result<(), CdPeelError> {
    let anchor = workspace_root
        .canonicalize()
        .map_err(|e| CdPeelError::SpawnOther {
            cmd: "canonicalize(workspace)".to_string(),
            source: e,
        })?;
    loop {
        if !cmd_raw.eq_ignore_ascii_case("cd") {
            break;
        }
        if cmd_args.len() < 3 || cmd_args[1] != "&&" {
            return Err(cd_prefix_invalid(
                effective_working_dir,
                "run_command 不经过 shell；`cd` 仅支持参数形式 [相对目录, \"&&\", 命令, …]，例如 [\"frontend\", \"&&\", \"cargo\", \"check\", …]",
            ));
        }
        let dir = cmd_args[0].trim();
        if !is_arg_safe("cd", dir) {
            return Err(CdPeelError::UnsafeArg);
        }
        let candidate = effective_working_dir.join(dir);
        if !candidate.is_dir() {
            return Err(cd_prefix_invalid(
                effective_working_dir,
                format!("路径 `{dir}` 不是已存在目录"),
            ));
        }
        let canon_cand = candidate
            .canonicalize()
            .map_err(|e| CdPeelError::SpawnOther {
                cmd: format!("canonicalize({})", candidate.display()),
                source: e,
            })?;
        if !canon_cand.starts_with(&anchor) {
            return Err(CdPeelError::UnsafeArg);
        }
        *effective_working_dir = canon_cand;
        *cmd_args = cmd_args[2..].to_vec();
        if cmd_args.is_empty() {
            return Err(CdPeelError::MissingCommand);
        }
        *cmd_raw = cmd_args[0].clone();
        let rest: Vec<String> = cmd_args[1..].to_vec();
        *cmd_args = rest;
        split_command_prefix_if_embedded(cmd_raw, cmd_args);
    }
    Ok(())
}
