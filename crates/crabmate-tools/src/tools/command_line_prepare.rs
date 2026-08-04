//! `run_command` 入参规范化：`command` 整段拆分、`cd <相对> && …` 前缀展开（无 shell）。

use std::io;
use std::path::{Path, PathBuf};

/// [`crate::tools::command::RunCommandError`] 在剥离 `cd` 前缀阶段的子集，避免 `command` ↔ 本模块循环依赖。
#[derive(Debug)]
pub enum CdPeelError {
    CdPrefixInvalid { detail: String, work_dir: String },
    UnsafeArg,
    MissingCommand,
    SpawnOther { cmd: String, source: io::Error },
}

/// 参数是否像路径穿越中的父目录引用（`..` / `../` / `/../` 等）。
///
/// **不**把 git 修订范围（如 `main..HEAD`、`a...b`）当成路径：它们含 `..` 子串但不是路径分量。
#[must_use]
pub fn arg_has_parent_dir_ref(arg: &str) -> bool {
    let a = arg.trim();
    if a.is_empty() {
        return false;
    }
    if a == ".." {
        return true;
    }
    if a.starts_with("../") || a.ends_with("/..") || a.contains("/../") {
        return true;
    }
    // `--exclude=../x` / `--opt=..`
    if a.contains("=../") || a.ends_with("=..") {
        return true;
    }
    // 纯路径分量：`foo/../bar` 已由 `/../` 覆盖；再兜底按 `/` 分段
    a.split('/').any(|c| c == "..")
}

pub fn is_arg_safe(cmd_name: &str, arg: &str) -> bool {
    let a = arg.trim();
    // cd 允许相对路径（禁止父目录穿越与绝对路径）
    if cmd_name == "cd" {
        return !arg_has_parent_dir_ref(a) && !a.starts_with('/');
    }
    // cmake 允许 .. (用于 cmake .. 从 build 目录配置源目录)
    if cmd_name == "cmake" {
        return !a.starts_with('/');
    }
    // 其他命令禁止父目录穿越与绝对路径（git `A..B` 范围不算穿越）
    !arg_has_parent_dir_ref(a) && !a.starts_with('/')
}

/// 将 `command: "./"` + 单个相对路径 `args`（模型按 shell 习惯误拆）合并为 `command: "./path"`。
///
/// 仅当 `command` 恰为 `./`、且唯一参数为不含 `..`/绝对路径的相对路径时生效；多参数或其它命令名不动。
pub fn merge_dot_slash_with_single_relative_path(cmd_raw: &mut String, cmd_args: &mut Vec<String>) {
    if cmd_raw.trim() != "./" {
        return;
    }
    if cmd_args.len() != 1 {
        return;
    }
    let arg = cmd_args[0].trim();
    if arg.is_empty() || arg_has_parent_dir_ref(arg) || arg.starts_with('/') {
        return;
    }
    *cmd_raw = if arg.starts_with("./") {
        arg.to_string()
    } else {
        format!("./{arg}")
    };
    cmd_args.clear();
}

/// 将 `command` 写成 `prog arg1 arg2` 整段而 `args` 为空（或需前缀拼接）的常见误用，规范为
/// `prog` + `["arg1","arg2", …原 args…]`，以便 [`std::process::Command::new`] 能解析到真实可执行文件。
///
/// 含 `/` 的值视为路径（含 `./` 与 `subdir/tool`），不做拆分，避免误伤带空格的可执行路径。
pub fn split_command_prefix_if_embedded(cmd_raw: &mut String, cmd_args: &mut Vec<String>) {
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

/// 将 `cd … && …` 前缀展开为嵌套工作目录与真实 argv（无 shell）。
///
/// 默认（`allow_external_paths == false`）：`rel` 不得含 `..` / 绝对路径，且规范路径须落在
/// `workspace_root` 之下。`allow_external_paths == true` 时跳过路径沙箱检查（仅人工审批通过后由
/// [`crate::tools::command`] 置位），仍要求目标为已存在目录。
pub fn peel_workspace_cd_prefix(
    workspace_root: &Path,
    effective_working_dir: &mut PathBuf,
    cmd_raw: &mut String,
    cmd_args: &mut Vec<String>,
    allow_external_paths: bool,
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
        if !allow_external_paths && !is_arg_safe("cd", dir) {
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
        if !allow_external_paths && !canon_cand.starts_with(&anchor) {
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

#[cfg(test)]
mod tests {
    use super::{arg_has_parent_dir_ref, is_arg_safe, merge_dot_slash_with_single_relative_path};

    #[test]
    fn parent_dir_ref_detects_path_traversal_not_git_range() {
        assert!(arg_has_parent_dir_ref(".."));
        assert!(arg_has_parent_dir_ref("../x"));
        assert!(arg_has_parent_dir_ref("a/../b"));
        assert!(arg_has_parent_dir_ref("a/.."));
        assert!(arg_has_parent_dir_ref("--exclude=../x"));
        assert!(arg_has_parent_dir_ref("--opt=.."));
        assert!(!arg_has_parent_dir_ref("main..HEAD"));
        assert!(!arg_has_parent_dir_ref("a...b"));
        assert!(!arg_has_parent_dir_ref("HEAD~1..HEAD"));
        assert!(!arg_has_parent_dir_ref("src"));
        assert!(!arg_has_parent_dir_ref(""));
    }

    #[test]
    fn is_arg_safe_allows_git_rev_range() {
        assert!(is_arg_safe("git", "main..HEAD"));
        assert!(is_arg_safe("git", "a...b"));
        assert!(!is_arg_safe("git", "../outside"));
        assert!(!is_arg_safe("ls", "/tmp"));
        assert!(is_arg_safe("cmake", ".."));
    }

    #[test]
    fn merge_dot_slash_single_relative_path() {
        let mut cmd = "./".to_string();
        let mut args = vec!["hello/build/hello".to_string()];
        merge_dot_slash_with_single_relative_path(&mut cmd, &mut args);
        assert_eq!(cmd, "./hello/build/hello");
        assert!(args.is_empty());
    }

    #[test]
    fn merge_dot_slash_preserves_arg_with_dot_slash_prefix() {
        let mut cmd = "./".to_string();
        let mut args = vec!["./bin/app".to_string()];
        merge_dot_slash_with_single_relative_path(&mut cmd, &mut args);
        assert_eq!(cmd, "./bin/app");
        assert!(args.is_empty());
    }

    #[test]
    fn merge_dot_slash_skips_multiple_args() {
        let mut cmd = "./".to_string();
        let mut args = vec!["a".to_string(), "b".to_string()];
        merge_dot_slash_with_single_relative_path(&mut cmd, &mut args);
        assert_eq!(cmd, "./");
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn merge_dot_slash_skips_unsafe_arg() {
        let mut cmd = "./".to_string();
        let mut args = vec!["../outside".to_string()];
        merge_dot_slash_with_single_relative_path(&mut cmd, &mut args);
        assert_eq!(cmd, "./");
        assert_eq!(args, vec!["../outside".to_string()]);
    }
}
