use std::path::Path;
use std::process::Command;

use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::output_util;

pub(super) const MAX_OUTPUT_LINES: usize = 800;

/// 「工作区不是 Git 仓库」的统一文案（[`ensure_git_repo_try`] 的两个分支共用）。
const NOT_A_REPO_MSG: &str = "错误：当前工作目录不在 Git 仓库中";

#[allow(clippy::result_large_err)]
pub(super) fn parse_args(args_json: &str) -> Result<serde_json::Value, ToolError> {
    crate::cm_tools::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)
}

#[allow(clippy::result_large_err)]
pub(super) fn extract_safe_path(v: &serde_json::Value) -> Result<Option<String>, ToolError> {
    match v.get("path").and_then(|x| x.as_str()) {
        Some(p) => {
            if is_safe_rel_path(p) {
                Ok(Some(p.trim().to_string()))
            } else {
                Err(ToolError::invalid_args(
                    "错误：path 必须是相对路径，且不能包含 \"..\" 或绝对路径".to_string(),
                ))
            }
        }
        None => Ok(None),
    }
}

#[allow(clippy::result_large_err)]
pub(super) fn require_safe_path(v: &serde_json::Value) -> Result<String, ToolError> {
    match v.get("path").and_then(|x| x.as_str()) {
        Some(p) if is_safe_rel_path(p) => Ok(p.trim().to_string()),
        _ => Err(ToolError::invalid_args(
            "错误：缺少合法 path 参数".to_string(),
        )),
    }
}

/// `Command::output()` → 工具正文；**非零退出即失败**（`error_code = failure_code`）。
///
/// `git` 的只读子命令里没有「非零退出但语义正常」的：需要看退出码的用法（`--exit-code` 等）
/// 属写操作、已收敛到 `run_command`；`status` / `branch` / `remote` 恒为 0。`log` / `show` /
/// `blame` 的 128 表示「无提交」或「rev / 路径不存在」——报失败并把 git 自己的 stderr 交回
/// 模型，比让它从正文里猜「这算不算成功」稳。
#[allow(clippy::result_large_err)]
pub(super) fn run_and_format(
    cmd: Command,
    max_output_len: usize,
    title: &str,
    failure_code: &str,
) -> Result<String, ToolError> {
    output_util::run_command_output_try(
        cmd,
        title,
        max_output_len,
        MAX_OUTPUT_LINES,
        output_util::ProcessOutputMerge::ConcatStdoutStderr,
        output_util::CommandSpawnErrorStyle::ExecuteFailed,
        failure_code,
    )
}

/// 统一处理 working/staged/all 三模式的 diff 类命令。
/// `extra_args` 为模式无关的附加参数（如 `--stat`、`--name-only`），
/// `context_fmt` 为可选 `-U{n}` 格式化字符串。
#[allow(clippy::result_large_err)]
pub(super) fn run_diff_mode(
    v: &serde_json::Value,
    max_output_len: usize,
    working_dir: &Path,
    extra_args: &[&str],
    context_fmt: Option<String>,
    title_base: &str,
) -> Result<String, ToolError> {
    let mode = v
        .get("mode")
        .and_then(|x| x.as_str())
        .unwrap_or("working")
        .trim()
        .to_lowercase();
    let path = extract_safe_path(v)?;

    let build_cmd = |staged: bool| {
        let mut cmd = Command::new("git");
        cmd.arg("diff");
        for a in extra_args {
            cmd.arg(*a);
        }
        if staged {
            cmd.arg("--staged");
        }
        if let Some(ref ctx) = context_fmt {
            cmd.arg(ctx);
        }
        if let Some(ref p) = path {
            cmd.arg("--").arg(p);
        }
        cmd.current_dir(working_dir);
        cmd
    };

    let title_suffix = |staged: bool| {
        if staged {
            format!("{} --staged", title_base)
        } else {
            title_base.to_string()
        }
    };

    match mode.as_str() {
        // `all` 的两次调用共用同一个码：对调用方而言这是一次 `git diff`，哪个模式失败都一样。
        "working" => run_and_format(
            build_cmd(false),
            max_output_len,
            &title_suffix(false),
            "git_diff_failed",
        ),
        "staged" => run_and_format(
            build_cmd(true),
            max_output_len,
            &title_suffix(true),
            "git_diff_failed",
        ),
        "all" => {
            let a = run_and_format(
                build_cmd(false),
                max_output_len,
                &title_suffix(false),
                "git_diff_failed",
            )?;
            let b = run_and_format(
                build_cmd(true),
                max_output_len,
                &title_suffix(true),
                "git_diff_failed",
            )?;
            Ok(format!("{}\n\n====================\n\n{}", a, b))
        }
        _ => Err(ToolError::invalid_args(
            "错误：mode 仅支持 working | staged | all".to_string(),
        )),
    }
}

/// 仓库前置检查：`git rev-parse --is-inside-work-tree` 须为 `true`。
///
/// 「起不来」归 `command_not_found`（带安装提示），「不在仓库里」归 `git_not_a_repo`
/// （`Workspace` 分类：换参数无用，须换目录）。
#[allow(clippy::result_large_err)]
pub(super) fn ensure_git_repo_try(working_dir: &Path) -> Result<(), ToolError> {
    let out = Command::new("git")
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .current_dir(working_dir)
        .output()
        .map_err(|e| {
            let b = format!("无法执行 git 命令: {}", e);
            ToolError::external_code(
                "command_not_found",
                output_util::append_notfound_install_hint(b, &e, "git"),
            )
        })?;
    if !out.status.success() || String::from_utf8_lossy(&out.stdout).trim() != "true" {
        return Err(ToolError::workspace(
            "git_not_a_repo",
            NOT_A_REPO_MSG.to_string(),
        ));
    }
    Ok(())
}

/// 旧的 `String` 版壳，供 [`super::super::github_cli`] 的 PR 正文草稿等既有调用点使用
/// （那边把失败包成自己的 `gh_git_failed`）。行为与 [`ensure_git_repo_try`] 一致。
pub fn ensure_git_repo(working_dir: &Path) -> Result<(), String> {
    ensure_git_repo_try(working_dir).map_err(|e| e.message)
}

pub(super) fn is_safe_rel_path(path: &str) -> bool {
    let p = path.trim();
    !p.is_empty() && !p.starts_with('/') && !p.contains("..")
}

#[cfg(test)]
mod tests {
    use super::is_safe_rel_path;

    #[test]
    fn test_is_safe_rel_path() {
        assert!(is_safe_rel_path("src/main.rs"));
        assert!(!is_safe_rel_path("/etc/passwd"));
        assert!(!is_safe_rel_path("../x"));
        assert!(!is_safe_rel_path(""));
    }
}
