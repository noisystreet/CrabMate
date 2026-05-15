use std::path::Path;
use std::process::Command;

use super::helpers::{
    MAX_OUTPUT_LINES, ensure_git_repo, is_safe_rel_path, parse_args, require_confirm,
    require_safe_path, require_string_field, run_and_format, run_diff_mode, section_failed,
};
use crate::tools::output_util;

/// `git diff` 正文常大于默认 `command_max_output_len`；在配置值之上保证至少本题字节预算，避免工具结果过早按字节截断。
const GIT_DIFF_MIN_CAP_BYTES: usize = 512 * 1024;

pub fn status(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
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
    run_and_format(cmd, max_output_len, "git status")
}

pub fn diff(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let context = v.get("context_lines").and_then(|x| x.as_u64()).unwrap_or(3);
    let cap = max_output_len.max(GIT_DIFF_MIN_CAP_BYTES);
    run_diff_mode(
        &v,
        cap,
        working_dir,
        &[],
        Some(format!("-U{}", context)),
        "git diff",
    )
}

pub fn clean_check(_args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let out = Command::new("git")
        .arg("status")
        .arg("--porcelain")
        .current_dir(working_dir)
        .output();
    match out {
        Ok(output) => {
            let status = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            if status != 0 {
                return format!(
                    "git clean check (exit={}):\n{}",
                    status,
                    output_util::truncate_output_lines(&stdout, max_output_len, MAX_OUTPUT_LINES)
                );
            }
            if stdout.trim().is_empty() {
                "git clean check (exit=0)：工作区干净".to_string()
            } else {
                format!(
                    "git clean check (exit=1)：存在未提交改动：\n{}",
                    output_util::truncate_output_lines(&stdout, max_output_len, MAX_OUTPUT_LINES)
                )
            }
        }
        Err(e) => format!("git clean check (exit=1)：执行失败：{}", e),
    }
}

pub fn diff_stat(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    run_diff_mode(
        &v,
        max_output_len,
        working_dir,
        &["--stat"],
        None,
        "git diff --stat",
    )
}

pub fn diff_names(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    run_diff_mode(
        &v,
        max_output_len,
        working_dir,
        &["--name-only"],
        None,
        "git diff --name-only",
    )
}

pub fn diff_base(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let base = v
        .get("base")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("main");
    let context = v.get("context_lines").and_then(|x| x.as_u64()).unwrap_or(3);
    let mut cmd = Command::new("git");
    cmd.arg("diff")
        .arg(format!("{}...HEAD", base))
        .arg(format!("-U{}", context))
        .current_dir(working_dir);
    let cap = max_output_len.max(GIT_DIFF_MIN_CAP_BYTES);
    run_and_format(cmd, cap, &format!("git diff {}...HEAD", base))
}

pub fn log(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let max_count = v.get("max_count").and_then(|x| x.as_u64()).unwrap_or(20);
    let oneline = v.get("oneline").and_then(|x| x.as_bool()).unwrap_or(true);
    let mut cmd = Command::new("git");
    cmd.arg("log").arg(format!("--max-count={}", max_count));
    if oneline {
        cmd.arg("--oneline");
    }
    cmd.current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git log")
}

pub fn show(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let rev = v
        .get("rev")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("HEAD");
    let mut cmd = Command::new("git");
    cmd.arg("show").arg(rev).current_dir(working_dir);
    run_and_format(cmd, max_output_len, &format!("git show {}", rev))
}

pub fn blame(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let path = match require_safe_path(&v) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let start = v.get("start_line").and_then(|x| x.as_u64());
    let end = v.get("end_line").and_then(|x| x.as_u64());
    let mut cmd = Command::new("git");
    cmd.arg("blame");
    if let (Some(s), Some(e)) = (start, end) {
        cmd.arg(format!("-L{},{}", s, e));
    }
    cmd.arg(&path).current_dir(working_dir);
    run_and_format(cmd, max_output_len, &format!("git blame {}", path))
}

pub fn file_history(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let path = match require_safe_path(&v) {
        Ok(p) => p,
        Err(e) => return e,
    };
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
    run_and_format(cmd, max_output_len, &format!("git log --follow {}", path))
}

pub fn branch_list(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
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
    run_and_format(cmd, max_output_len, "git branch")
}

pub fn remote_status(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let _v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let mut cmd = Command::new("git");
    cmd.arg("status").arg("-sb").current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git status -sb")
}

pub fn remote_list(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let _v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let mut cmd = Command::new("git");
    cmd.arg("remote").arg("-v").current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git remote -v")
}

pub fn remote_set_url(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    if let Err(e) = require_confirm(&v, "git_remote_set_url") {
        return e;
    }
    let name = match require_string_field(&v, "name") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let url = match require_string_field(&v, "url") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut cmd = Command::new("git");
    cmd.arg("remote")
        .arg("set-url")
        .arg(name)
        .arg(url)
        .current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git remote set-url")
}

pub fn fetch(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let remote = v.get("remote").and_then(|x| x.as_str()).map(str::trim);
    let branch = v.get("branch").and_then(|x| x.as_str()).map(str::trim);
    let prune = v.get("prune").and_then(|x| x.as_bool()).unwrap_or(false);
    let mut cmd = Command::new("git");
    cmd.arg("fetch");
    if prune {
        cmd.arg("--prune");
    }
    if let Some(r) = remote.filter(|s| !s.is_empty()) {
        cmd.arg(r);
        if let Some(b) = branch.filter(|s| !s.is_empty()) {
            cmd.arg(b);
        }
    }
    cmd.current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git fetch")
}

pub fn apply(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let patch = match v.get("patch_path").and_then(|x| x.as_str()) {
        Some(p) if is_safe_rel_path(p) => p.trim(),
        _ => return "错误：缺少合法 patch_path 参数".to_string(),
    };
    let check_only = v
        .get("check_only")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let mut cmd = Command::new("git");
    cmd.arg("apply");
    if check_only {
        cmd.arg("--check");
    }
    cmd.arg("--").arg(patch).current_dir(working_dir);
    run_and_format(
        cmd,
        max_output_len,
        if check_only {
            "git apply --check"
        } else {
            "git apply"
        },
    )
}

pub fn clone_repo(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = require_confirm(&v, "git_clone") {
        return e;
    }
    let repo_url = match require_string_field(&v, "repo_url") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let target_dir = match v.get("target_dir").and_then(|x| x.as_str()) {
        Some(p) if is_safe_rel_path(p) => p.trim(),
        _ => return "错误：缺少合法 target_dir 参数（必须是相对路径）".to_string(),
    };
    let depth = v.get("depth").and_then(|x| x.as_u64());
    let base = match working_dir.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作目录无法解析: {}", e),
    };
    let target_abs = base.join(target_dir);
    if target_abs.exists() {
        return "错误：target_dir 已存在".to_string();
    }
    let mut cmd = Command::new("git");
    cmd.arg("clone");
    if let Some(d) = depth {
        cmd.arg("--depth").arg(d.to_string());
    }
    cmd.arg(repo_url).arg(target_dir).current_dir(&base);
    run_and_format(cmd, max_output_len, "git clone")
}

pub fn stage_files(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let files = match v.get("paths").and_then(|x| x.as_array()) {
        Some(arr) if !arr.is_empty() => arr
            .iter()
            .filter_map(|x| x.as_str())
            .map(str::trim)
            .filter(|p| is_safe_rel_path(p))
            .map(str::to_string)
            .collect::<Vec<_>>(),
        _ => return "错误：paths 必须是非空字符串数组".to_string(),
    };
    if files.is_empty() {
        return "错误：paths 中没有合法相对路径".to_string();
    }
    let mut cmd = Command::new("git");
    cmd.arg("add").arg("--");
    for p in files {
        cmd.arg(p);
    }
    cmd.current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git add")
}

pub fn commit(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    if let Err(e) = require_confirm(&v, "git_commit") {
        return format!("{}才会真正提交", e);
    }
    let message = match require_string_field(&v, "message") {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let stage_all = v
        .get("stage_all")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    if stage_all {
        let mut add = Command::new("git");
        add.arg("add").arg("-A").current_dir(working_dir);
        let out = run_and_format(add, max_output_len, "git add -A");
        if section_failed(&out) {
            return out;
        }
    }
    let mut cmd = Command::new("git");
    cmd.arg("commit")
        .arg("-m")
        .arg(message)
        .current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git commit")
}
