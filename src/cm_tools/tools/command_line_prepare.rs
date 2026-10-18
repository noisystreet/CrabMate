//! `run_command` 入参规范化：`command` 整段拆分、`cd <相对> && …` 前缀展开（无 shell）。

use std::io;
use std::path::{Path, PathBuf};

use super::command::is_posix_shell_name;

/// [`crate::cm_tools::tools::command::RunCommandError`] 在剥离 `cd` 前缀阶段的子集，避免 `command` ↔ 本模块循环依赖。
#[derive(Debug)]
pub enum CdPeelError {
    CdPrefixInvalid { detail: String, work_dir: String },
    UnsafeArg,
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

/// 将 `command: "./"` + 首个相对路径 `args[0]`（模型按 shell 习惯误拆）合并为 `command: "./path"`，
/// 其余参数保留给程序。
///
/// 仅当 `command` 恰为 `./`、且首个参数为不含 `..`/绝对路径、且不以 `-` 开头（避免误把选项当成路径）的相对路径时生效。
pub fn merge_dot_slash_with_single_relative_path(cmd_raw: &mut String, cmd_args: &mut Vec<String>) {
    if cmd_raw.trim() != "./" {
        return;
    }
    let Some(first) = cmd_args.first() else {
        return;
    };
    let arg = first.trim();
    if arg.is_empty() || arg.starts_with('-') || arg_has_parent_dir_ref(arg) || arg.starts_with('/')
    {
        return;
    }
    *cmd_raw = if arg.starts_with("./") {
        arg.to_string()
    } else {
        format!("./{arg}")
    };
    cmd_args.remove(0);
}

/// 将 `command` 写成 `prog arg1 arg2` 整段而 `args` 为空（或需前缀拼接）的常见误用，规范为
/// `prog` + `["arg1","arg2", …原 args…]`，以便 [`std::process::Command::new`] 能解析到真实可执行文件。
///
/// 含 `/` 的值视为路径（含 `./` 与 `subdir/tool`），不做拆分，避免误伤带空格的可执行路径；
/// 但 `bash -c` / `sh -c`（脚本正文允许含 `/`）在此检查之前按 shell 语义处理。
pub fn split_command_prefix_if_embedded(cmd_raw: &mut String, cmd_args: &mut Vec<String>) {
    if let Some((head, mut prefix)) = split_embedded_shell_script(cmd_raw) {
        prefix.append(cmd_args);
        *cmd_args = prefix;
        *cmd_raw = head;
        return;
    }
    if cmd_raw.contains('/') {
        return;
    }
    let parts = crate::cmd_mate::split_command_line(cmd_raw);
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

/// 整段 `command` 写成 `bash -c '<脚本>'` 时，按 shell 语义拆分并**原样保留**脚本正文。
///
/// POSIX 分词会消费 `\$` / `\\` 并丢弃引号，使脚本内的转义失效；此处只剥离一层完整包裹的外层引号，
/// 其余字符（含反斜杠、`$`、换行）保持原样。返回 `(程序名, 脚本前的 argv)`。
fn split_embedded_shell_script(cmd_raw: &str) -> Option<(String, Vec<String>)> {
    let (head, mut rest) = split_first_token(cmd_raw)?;
    if !is_posix_shell_name(head) {
        return None;
    }
    let mut prefix: Vec<String> = Vec::new();
    loop {
        let (tok, tail) = split_first_token(rest)?;
        if tok == "--" || !tok.starts_with('-') {
            return None;
        }
        if tok.starts_with("--") || !tok.contains('c') {
            prefix.push(tok.to_string());
            rest = tail;
            continue;
        }
        let script = strip_one_pair_of_outer_quotes(tail.trim());
        if script.is_empty() {
            return None;
        }
        prefix.push(tok.to_string());
        prefix.push(script.to_string());
        return Some((head.to_string(), prefix));
    }
}

fn split_first_token(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    Some((&s[..end], &s[end..]))
}

/// 若 `s` 由一对引号**完整包裹**（引号内部不再提前闭合），返回去掉这层引号的内容；否则原样返回。
///
/// 双引号内按 shell 规则跳过 `\X` 转义；转义字符本身不还原，调用方保持原样即可。
fn strip_one_pair_of_outer_quotes(s: &str) -> &str {
    let Some(open @ ('\'' | '"')) = s.chars().next() else {
        return s;
    };
    if s.len() < 2 || !s.ends_with(open) {
        return s;
    }
    let inner = &s[1..s.len() - 1];
    if open == '\'' {
        return if inner.contains('\'') { s } else { inner };
    }
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return s;
        }
    }
    inner
}

/// 识别一层 `cd <dir> && <rest…>`（`args` 形如 `[dir, "&&", cmd, rest…]`）。
///
/// 仅做纯字符串判定；是否接受该目录、如何推进工作目录由各调用方决定。
#[must_use]
pub fn parse_cd_prefix(args: &[String]) -> Option<(&str, &[String])> {
    if args.len() < 3 || args[1] != "&&" {
        return None;
    }
    Some((args[0].trim(), &args[2..]))
}

fn cd_prefix_invalid(work_dir: &Path, detail: impl Into<String>) -> CdPeelError {
    CdPeelError::CdPrefixInvalid {
        detail: detail.into(),
        work_dir: work_dir.display().to_string(),
    }
}

pub(crate) fn resolve_cd_prefix_dir(
    effective_working_dir: &Path,
    dir: &str,
    anchor: &Path,
    allow_external_paths: bool,
) -> Result<PathBuf, CdPeelError> {
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
    if !allow_external_paths && !canon_cand.starts_with(anchor) {
        return Err(CdPeelError::UnsafeArg);
    }
    Ok(canon_cand)
}

/// 将 `cd … && …` 前缀展开为嵌套工作目录与真实 argv（无 shell）。
///
/// 默认（`allow_external_paths == false`）：`rel` 不得含 `..` / 绝对路径，且规范路径须落在
/// `workspace_root` 之下。`allow_external_paths == true` 时跳过路径沙箱检查（仅人工审批通过后由
/// [`crate::cm_tools::tools::command`] 置位），仍要求目标为已存在目录。
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
        let Some((dir, rest)) = parse_cd_prefix(cmd_args) else {
            return Err(cd_prefix_invalid(
                effective_working_dir,
                "run_command 不经过 shell；`cd` 仅支持参数形式 [相对目录, \"&&\", 命令, …]，例如 [\"frontend\", \"&&\", \"cargo\", \"check\", …]",
            ));
        };
        let dir = dir.to_string();
        let mut rest = rest.to_vec();
        *effective_working_dir =
            resolve_cd_prefix_dir(effective_working_dir, &dir, &anchor, allow_external_paths)?;
        *cmd_raw = rest.remove(0);
        *cmd_args = rest;
        split_command_prefix_if_embedded(cmd_raw, cmd_args);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        arg_has_parent_dir_ref, is_arg_safe, merge_dot_slash_with_single_relative_path,
        split_command_prefix_if_embedded,
    };

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
    fn merge_dot_slash_merges_leading_path_keeps_rest_args() {
        let mut cmd = "./".to_string();
        let mut args = vec![
            "build/hello".to_string(),
            "-g".to_string(),
            "Hi".to_string(),
            "-n".to_string(),
            "2".to_string(),
            "Alice".to_string(),
        ];
        merge_dot_slash_with_single_relative_path(&mut cmd, &mut args);
        assert_eq!(cmd, "./build/hello");
        assert_eq!(
            args,
            vec![
                "-g".to_string(),
                "Hi".to_string(),
                "-n".to_string(),
                "2".to_string(),
                "Alice".to_string(),
            ]
        );
    }

    #[test]
    fn merge_dot_slash_skips_when_first_arg_is_flag() {
        let mut cmd = "./".to_string();
        let mut args = vec!["-g".to_string(), "Hi".to_string()];
        merge_dot_slash_with_single_relative_path(&mut cmd, &mut args);
        assert_eq!(cmd, "./");
        assert_eq!(args, vec!["-g".to_string(), "Hi".to_string()]);
    }

    #[test]
    fn merge_dot_slash_skips_unsafe_arg() {
        let mut cmd = "./".to_string();
        let mut args = vec!["../outside".to_string()];
        merge_dot_slash_with_single_relative_path(&mut cmd, &mut args);
        assert_eq!(cmd, "./");
        assert_eq!(args, vec!["../outside".to_string()]);
    }

    #[test]
    fn embedded_bash_dash_c_preserves_escaped_dollar() {
        let mut cmd = r#"bash -c "echo \$HOME""#.to_string();
        let mut args = vec![];
        split_command_prefix_if_embedded(&mut cmd, &mut args);
        assert_eq!(cmd, "bash");
        assert_eq!(args, vec!["-c".to_string(), r#"echo \$HOME"#.to_string()]);
    }

    #[test]
    fn embedded_bash_dash_c_preserves_single_quoted_script() {
        let mut cmd = r#"bash -c 'printf "%s\n" x'"#.to_string();
        let mut args = vec![];
        split_command_prefix_if_embedded(&mut cmd, &mut args);
        assert_eq!(cmd, "bash");
        assert_eq!(
            args,
            vec!["-c".to_string(), r#"printf "%s\n" x"#.to_string()]
        );
    }

    #[test]
    fn embedded_sh_combined_flags_keep_script() {
        let mut cmd = "sh -lc 'cd sub && ls'".to_string();
        let mut args = vec![];
        split_command_prefix_if_embedded(&mut cmd, &mut args);
        assert_eq!(cmd, "sh");
        assert_eq!(
            args,
            vec!["-lc".to_string(), "cd sub && ls".to_string()]
        );
    }

    #[test]
    fn embedded_bash_keeps_long_options_before_dash_c() {
        let mut cmd = "bash --norc -c 'echo $HOME'".to_string();
        let mut args = vec![];
        split_command_prefix_if_embedded(&mut cmd, &mut args);
        assert_eq!(cmd, "bash");
        assert_eq!(
            args,
            vec![
                "--norc".to_string(),
                "-c".to_string(),
                "echo $HOME".to_string(),
            ]
        );
    }

    #[test]
    fn embedded_bash_dash_c_keeps_quotes_not_wrapping_whole_script() {
        let mut cmd = r#"bash -c "echo hi" more"#.to_string();
        let mut args = vec![];
        split_command_prefix_if_embedded(&mut cmd, &mut args);
        assert_eq!(cmd, "bash");
        assert_eq!(
            args,
            vec!["-c".to_string(), r#""echo hi" more"#.to_string()]
        );
    }

    #[test]
    fn embedded_bash_dash_c_script_may_contain_slash() {
        let mut cmd = r#"bash -c "ls /tmp && echo \$HOME""#.to_string();
        let mut args = vec![];
        split_command_prefix_if_embedded(&mut cmd, &mut args);
        assert_eq!(cmd, "bash");
        assert_eq!(
            args,
            vec!["-c".to_string(), r#"ls /tmp && echo \$HOME"#.to_string()]
        );
    }

    #[test]
    fn path_with_slash_is_not_split_unless_shell_dash_c() {
        let mut cmd = "./build/hello -g Hi".to_string();
        let mut args = vec![];
        split_command_prefix_if_embedded(&mut cmd, &mut args);
        assert_eq!(cmd, "./build/hello -g Hi");
        assert!(args.is_empty());
    }

    #[test]
    fn non_shell_command_still_uses_posix_split() {
        let mut cmd = r#"git log -5 --format="%h %ai""#.to_string();
        let mut args = vec![];
        split_command_prefix_if_embedded(&mut cmd, &mut args);
        assert_eq!(cmd, "git");
        assert_eq!(
            args,
            vec![
                "log".to_string(),
                "-5".to_string(),
                "--format=%h %ai".to_string(),
            ]
        );
    }
}
