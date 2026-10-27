use std::path::Path;
use std::process::Command;

use super::helpers::{
    MAX_OUTPUT_LINES, ensure_git_repo_try, extract_safe_path, parse_args, require_safe_path,
    run_and_format, run_diff_mode,
};
use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::output_util;
use crate::cm_tools::tools::tool_param_types::GitAction;

/// `git diff` 正文常大于默认 `command_max_output_len`；在配置值之上保证至少本题字节预算，避免工具结果过早按字节截断。
const GIT_DIFF_MIN_CAP_BYTES: usize = 512 * 1024;

/// `git` 工具的 action 可选值（错误提示与 spec description 共用一份）。
const GIT_ACTIONS: &str = "status / diff / clean_check / log / show / blame / \
file_history / branch_list / remote_status / remote_list";

/// `git` 工具统一入口：按入参 `action` 分发到对应只读查询。
///
/// 合并自原 10 个 `git_*` 工具——它们都是同一个 `git` CLI 的只读子命令包装，
/// 合并只改变了「从哪里拿到要跑哪个子命令」。各查询仍自行 `parse_args` 取所需字段，
/// 因此 `action` 之外的字段被忽略，仓库校验与错误文案与逐工具时代一致。
///
/// 分发**经由 `GitAction` 反序列化**而不是匹配字符串字面量：`GitAction` 同时是 schema 的
/// enum 真源，新增/改名 action 时下面的 `match` 不穷尽会直接编译失败，避免「schema 放行、
/// 运行时却报未知 action」这种只改一半的静默不一致。
///
/// 失败语义（`Typed` runner）：参数 / `action` / `path` / `mode` → `invalid_args`；
/// 工作目录不是 Git 仓库 → `git_not_a_repo`；本机无 `git` → `command_not_found`；
/// 子命令**非零退出** → `git_{action}_failed`（`status` / `diff` / `clean_check` / `log` /
/// `show` / `blame` / `file_history` / `branch_list` / `remote_status` / `remote_list`）。
#[allow(clippy::result_large_err)]
pub fn dispatch(
    args_json: &str,
    max_output_len: usize,
    working_dir: &Path,
) -> Result<String, ToolError> {
    let v = parse_args(args_json)?;
    let unknown = |name: &str| format!("错误：未知 action `{name}`（可选值：{GIT_ACTIONS}）");
    let raw = v.get("action").cloned().unwrap_or(serde_json::Value::Null);
    let Some(name) = raw.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
        return Err(ToolError::invalid_args(format!(
            "错误：缺少 action（可选值：{GIT_ACTIONS}）"
        )));
    };
    // 先取出原值再反序列化，失败时能把拼错的 action 原样回显给调用方。
    let action: GitAction = match serde_json::from_value(serde_json::Value::String(name.to_string())) {
        Ok(a) => a,
        Err(_) => return Err(ToolError::invalid_args(unknown(name))),
    };
    match action {
        GitAction::Status => status(args_json, max_output_len, working_dir),
        GitAction::Diff => diff(args_json, max_output_len, working_dir),
        GitAction::CleanCheck => clean_check(args_json, max_output_len, working_dir),
        GitAction::Log => log(args_json, max_output_len, working_dir),
        GitAction::Show => show(args_json, max_output_len, working_dir),
        GitAction::Blame => blame(args_json, max_output_len, working_dir),
        GitAction::FileHistory => file_history(args_json, max_output_len, working_dir),
        GitAction::BranchList => branch_list(args_json, max_output_len, working_dir),
        GitAction::RemoteStatus => remote_status(args_json, max_output_len, working_dir),
        GitAction::RemoteList => remote_list(args_json, max_output_len, working_dir),
    }
}

#[allow(clippy::result_large_err)]
fn status(
    args_json: &str,
    max_output_len: usize,
    working_dir: &Path,
) -> Result<String, ToolError> {
    let v = parse_args(args_json)?;
    ensure_git_repo_try(working_dir)?;
    let porcelain = v
        .get("porcelain")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let include_untracked = v
        .get("include_untracked")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let show_branch = v.get("branch").and_then(|x| x.as_bool()).unwrap_or(true);

    let mut cmd = Command::new("git");
    cmd.arg("status");
    if porcelain {
        cmd.arg("--porcelain");
    }
    if show_branch {
        cmd.arg("--branch");
    }
    if !include_untracked {
        cmd.arg("--untracked-files=no");
    }
    cmd.current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git status", "git_status_failed")
}

#[allow(clippy::result_large_err)]
fn diff(args_json: &str, max_output_len: usize, working_dir: &Path) -> Result<String, ToolError> {
    let v = parse_args(args_json)?;
    ensure_git_repo_try(working_dir)?;
    let cap = max_output_len.max(GIT_DIFF_MIN_CAP_BYTES);
    let context = v.get("context_lines").and_then(|x| x.as_u64()).unwrap_or(3);
    let mut extra: Vec<&str> = Vec::new();
    if v.get("stat").and_then(|x| x.as_bool()).unwrap_or(false) {
        extra.push("--stat");
    }
    if v.get("name_only").and_then(|x| x.as_bool()).unwrap_or(false) {
        extra.push("--name-only");
    }
    let title_base = if extra.is_empty() {
        "git diff".to_string()
    } else {
        format!("git diff {}", extra.join(" "))
    };
    // base 存在时改为对比 base...HEAD 范围（忽略 mode）。
    if let Some(base) = v
        .get("base")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let path = extract_safe_path(&v)?;
        let mut cmd = Command::new("git");
        cmd.arg("diff").arg(format!("{}...HEAD", base));
        for a in &extra {
            cmd.arg(*a);
        }
        cmd.arg(format!("-U{}", context));
        if let Some(ref p) = path {
            cmd.arg("--").arg(p);
        }
        cmd.current_dir(working_dir);
        return run_and_format(
            cmd,
            cap,
            &format!("git diff {}...HEAD", base),
            "git_diff_failed",
        );
    }
    run_diff_mode(
        &v,
        cap,
        working_dir,
        &extra,
        Some(format!("-U{}", context)),
        &title_base,
    )
}

#[allow(clippy::result_large_err)]
fn clean_check(
    _args_json: &str,
    max_output_len: usize,
    working_dir: &Path,
) -> Result<String, ToolError> {
    ensure_git_repo_try(working_dir)?;
    let out = Command::new("git")
        .arg("status")
        .arg("--porcelain")
        .current_dir(working_dir)
        .output()
        .map_err(|e| {
            let base = format!("git clean check (exit=1)：执行失败：{}", e);
            ToolError::external_code(
                "git_clean_check_failed",
                output_util::append_notfound_install_hint(base, &e, "git"),
            )
        })?;
    let status = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let body = output_util::truncate_output_lines(&stdout, max_output_len, MAX_OUTPUT_LINES);
    if status != 0 {
        // 正文形如旧实现的失败分支；判定改为显式 `Err`（以前只有 text，靠关键词猜）。
        return Err(ToolError::external_code(
            "git_clean_check_failed",
            format!("git clean check (exit={}):\n{}", status, body),
        ));
    }
    if stdout.trim().is_empty() {
        Ok("git clean check (exit=0)：工作区干净".to_string())
    } else {
        // `exit=1` 是历史文案（语义是「有未提交改动」），保持不变以免下游字符串匹配失配。
        Ok(format!(
            "git clean check (exit=1)：存在未提交改动：\n{}",
            body
        ))
    }
}

#[allow(clippy::result_large_err)]
fn log(args_json: &str, max_output_len: usize, working_dir: &Path) -> Result<String, ToolError> {
    let v = parse_args(args_json)?;
    ensure_git_repo_try(working_dir)?;
    let max_count = v.get("max_count").and_then(|x| x.as_u64()).unwrap_or(20);
    let oneline = v.get("oneline").and_then(|x| x.as_bool()).unwrap_or(true);
    let mut cmd = Command::new("git");
    cmd.arg("log").arg(format!("--max-count={}", max_count));
    if oneline {
        cmd.arg("--oneline");
    }
    cmd.current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git log", "git_log_failed")
}

#[allow(clippy::result_large_err)]
fn show(args_json: &str, max_output_len: usize, working_dir: &Path) -> Result<String, ToolError> {
    let v = parse_args(args_json)?;
    ensure_git_repo_try(working_dir)?;
    let rev = v
        .get("rev")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("HEAD");
    let mut cmd = Command::new("git");
    cmd.arg("show").arg(rev).current_dir(working_dir);
    run_and_format(cmd, max_output_len, &format!("git show {}", rev), "git_show_failed")
}

#[allow(clippy::result_large_err)]
fn blame(args_json: &str, max_output_len: usize, working_dir: &Path) -> Result<String, ToolError> {
    let v = parse_args(args_json)?;
    ensure_git_repo_try(working_dir)?;
    let path = require_safe_path(&v)?;
    let start = v.get("start_line").and_then(|x| x.as_u64());
    let end = v.get("end_line").and_then(|x| x.as_u64());
    let mut cmd = Command::new("git");
    cmd.arg("blame");
    if let (Some(s), Some(e)) = (start, end) {
        cmd.arg(format!("-L{},{}", s, e));
    }
    cmd.arg(&path).current_dir(working_dir);
    run_and_format(
        cmd,
        max_output_len,
        &format!("git blame {}", path),
        "git_blame_failed",
    )
}

#[allow(clippy::result_large_err)]
fn file_history(
    args_json: &str,
    max_output_len: usize,
    working_dir: &Path,
) -> Result<String, ToolError> {
    let v = parse_args(args_json)?;
    ensure_git_repo_try(working_dir)?;
    let path = require_safe_path(&v)?;
    let max_count = v.get("max_count").and_then(|x| x.as_u64()).unwrap_or(30);
    let mut cmd = Command::new("git");
    cmd.arg("log")
        .arg("--follow")
        .arg("--name-status")
        .arg("--oneline")
        .arg(format!("--max-count={}", max_count))
        .arg("--")
        .arg(&path)
        .current_dir(working_dir);
    run_and_format(
        cmd,
        max_output_len,
        &format!("git log --follow {}", path),
        "git_file_history_failed",
    )
}

#[allow(clippy::result_large_err)]
fn branch_list(
    args_json: &str,
    max_output_len: usize,
    working_dir: &Path,
) -> Result<String, ToolError> {
    let v = parse_args(args_json)?;
    ensure_git_repo_try(working_dir)?;
    let include_remote = v
        .get("include_remote")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let mut cmd = Command::new("git");
    cmd.arg("branch");
    if include_remote {
        cmd.arg("-a");
    }
    cmd.current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git branch", "git_branch_list_failed")
}

#[allow(clippy::result_large_err)]
fn remote_status(
    args_json: &str,
    max_output_len: usize,
    working_dir: &Path,
) -> Result<String, ToolError> {
    let _v = parse_args(args_json)?;
    ensure_git_repo_try(working_dir)?;
    let mut cmd = Command::new("git");
    cmd.arg("status").arg("-sb").current_dir(working_dir);
    run_and_format(
        cmd,
        max_output_len,
        "git status -sb",
        "git_remote_status_failed",
    )
}

#[allow(clippy::result_large_err)]
fn remote_list(
    args_json: &str,
    max_output_len: usize,
    working_dir: &Path,
) -> Result<String, ToolError> {
    let _v = parse_args(args_json)?;
    ensure_git_repo_try(working_dir)?;
    let mut cmd = Command::new("git");
    cmd.arg("remote").arg("-v").current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git remote -v", "git_remote_list_failed")
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::dispatch;

    /// 在临时目录里初始化一个空仓库（无提交）。测试依赖本机 `git`。
    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let ok = Command::new("git")
            .arg("init")
            .arg("-q")
            .current_dir(dir.path())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "本测试需要可用的 git 可执行文件");
        dir
    }

    /// `action` 必须显式给出：合并前是 10 个各自独立的工具名，缺参不会有歧义。
    #[test]
    fn dispatch_rejects_missing_action() {
        let e = dispatch("{}", 4096, std::path::Path::new(".")).expect_err("缺 action 应失败");
        assert_eq!(e.code, "invalid_args");
        assert!(e.message.contains("缺少 action"), "{}", e.message);
        assert!(e.message.contains("clean_check"), "错误应列出可选值：{}", e.message);
    }

    /// 未知 action 不能静默落到某个默认子命令上。
    #[test]
    fn dispatch_rejects_unknown_action() {
        let e = dispatch(r#"{"action":"nope"}"#, 4096, std::path::Path::new("."))
            .expect_err("未知 action 应失败");
        assert_eq!(e.code, "invalid_args");
        assert!(e.message.contains("未知 action"), "{}", e.message);
    }

    /// 每个 action 都应被真正转发到对应子命令（这里用非仓库临时目录，
    /// 命中各查询共有的仓库校验——只要不是「未知/缺少 action」，就说明分发命中）。
    #[test]
    fn dispatch_forwards_every_action() {
        let dir = tempfile::tempdir().expect("tempdir");
        for action in [
            "status",
            "diff",
            "clean_check",
            "log",
            "show",
            "blame",
            "file_history",
            "branch_list",
            "remote_status",
            "remote_list",
        ] {
            let args = format!(r#"{{"action":"{action}"}}"#);
            let e = dispatch(&args, 4096, dir.path()).expect_err("非仓库目录应失败");
            assert_ne!(
                e.code, "invalid_args",
                "{action} 应被分发给具体查询而不是被拒绝：{}",
                e.message
            );
        }
    }

    /// 不在 Git 仓库里：显式 `git_not_a_repo`（`Workspace` 分类），不再是只能靠正文判断。
    #[test]
    fn non_repo_directory_reports_git_not_a_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let e = dispatch(r#"{"action":"status"}"#, 4096, dir.path())
            .expect_err("非仓库目录应失败");
        assert_eq!(e.code, "git_not_a_repo");
        assert!(e.message.contains("不在 Git 仓库中"), "{}", e.message);
        assert_eq!(
            e.category,
            crate::cm_tools::tool_result::ToolFailureCategory::Workspace
        );
    }

    /// 成功路径的正文与旧实现逐字一致（只有失败路径从「文本猜」变成 `Err`）。
    #[test]
    fn status_on_repository_is_success() {
        let dir = init_repo();
        let out = dispatch(r#"{"action":"status"}"#, 4096, dir.path()).expect("status 应成功");
        assert!(out.starts_with("git status (exit=0):"), "{out}");
    }

    /// 非零退出必须变成 `git_{action}_failed`：`show` 一个不存在的 rev 是 128。
    #[test]
    fn nonzero_exit_reports_git_action_failed() {
        let dir = init_repo();
        let e = dispatch(r#"{"action":"show","rev":"deadbeef"}"#, 4096, dir.path())
            .expect_err("不存在的 rev 应失败");
        assert_eq!(e.code, "git_show_failed");
        // git 自己的 stderr 仍在正文里（模型可据此判断是 rev 不存在）。
        assert!(e.message.contains("git show deadbeef"), "{}", e.message);
    }

    /// 空仓库（尚无提交）的 `log` 同样是 128：以前只有正文「exit=128」，被启发式判成功。
    #[test]
    fn log_in_empty_repository_is_a_failure() {
        let dir = init_repo();
        let e = dispatch(r#"{"action":"log"}"#, 4096, dir.path())
            .expect_err("空仓库 log 应显式失败");
        assert_eq!(e.code, "git_log_failed");
    }

    /// 有提交后 `log` 恢复正常 `Ok`，确认上一条不是把正常路径判坏。
    #[test]
    fn log_after_first_commit_is_success() {
        let dir = init_repo();
        let ok = Command::new("git")
            .args([
                "-c",
                "user.name=CrabMate Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "init",
                "-q",
            ])
            .current_dir(dir.path())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "首次提交失败");
        let out = dispatch(r#"{"action":"log"}"#, 4096, dir.path()).expect("log 应成功");
        assert!(out.starts_with("git log (exit=0):"), "{out}");
        assert!(out.contains("init"), "{out}");
    }
}
