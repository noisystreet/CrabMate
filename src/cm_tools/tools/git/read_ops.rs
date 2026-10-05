use std::path::Path;
use std::process::Command;

use super::helpers::{
    MAX_OUTPUT_LINES, ensure_git_repo, extract_safe_path, parse_args, require_safe_path,
    run_and_format, run_diff_mode,
};
use crate::cm_tools::tools::output_util;

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
        let path = match extract_safe_path(&v) {
            Ok(p) => p,
            Err(e) => return e,
        };
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
        return run_and_format(cmd, cap, &format!("git diff {}...HEAD", base));
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
