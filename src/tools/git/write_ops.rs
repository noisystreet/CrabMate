use std::path::Path;
use std::process::Command;

use super::helpers::{
    ensure_git_repo, parse_args, require_confirm, require_string_field, run_and_format,
};

pub fn checkout(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let target = match require_string_field(&v, "target") {
        Ok(s) => s,
        Err(e) => return e,
    };
    if target.contains("..") {
        return "错误：target 不能包含 ..".to_string();
    }
    let create = v.get("create").and_then(|x| x.as_bool()).unwrap_or(false);
    let mut cmd = Command::new("git");
    if create {
        cmd.arg("checkout").arg("-b").arg(target);
    } else {
        cmd.arg("checkout").arg(target);
    }
    cmd.current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git checkout")
}

pub fn branch_create(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let name = match require_string_field(&v, "name") {
        Ok(s) => s,
        Err(e) => return e,
    };
    if name.contains("..") || name.starts_with('-') {
        return "错误：分支名不合法".to_string();
    }
    let start_point = v
        .get("start_point")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let mut cmd = Command::new("git");
    cmd.arg("branch").arg(name);
    if let Some(sp) = start_point {
        cmd.arg(sp);
    }
    cmd.current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git branch（创建）")
}

pub fn branch_delete(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    if let Err(e) = require_confirm(&v, "git_branch_delete") {
        return e;
    }
    let name = match require_string_field(&v, "name") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let force = v.get("force").and_then(|x| x.as_bool()).unwrap_or(false);
    let mut cmd = Command::new("git");
    cmd.arg("branch");
    if force {
        cmd.arg("-D");
    } else {
        cmd.arg("-d");
    }
    cmd.arg(name).current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git branch（删除）")
}

pub fn push(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    if let Err(e) = require_confirm(&v, "git_push") {
        return e;
    }
    let remote = v
        .get("remote")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("origin");
    let branch = v
        .get("branch")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let set_upstream = v
        .get("set_upstream")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let force_with_lease = v
        .get("force_with_lease")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let tags = v.get("tags").and_then(|x| x.as_bool()).unwrap_or(false);

    let mut cmd = Command::new("git");
    cmd.arg("push");
    if set_upstream {
        cmd.arg("-u");
    }
    if force_with_lease {
        cmd.arg("--force-with-lease");
    }
    if tags {
        cmd.arg("--tags");
    }
    cmd.arg(remote);
    if let Some(b) = branch {
        cmd.arg(b);
    }
    cmd.current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git push")
}

pub fn merge(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    if let Err(e) = require_confirm(&v, "git_merge") {
        return e;
    }
    let branch = match require_string_field(&v, "branch") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let no_ff = v.get("no_ff").and_then(|x| x.as_bool()).unwrap_or(false);
    let squash = v.get("squash").and_then(|x| x.as_bool()).unwrap_or(false);
    let message = v
        .get("message")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut cmd = Command::new("git");
    cmd.arg("merge");
    if no_ff {
        cmd.arg("--no-ff");
    }
    if squash {
        cmd.arg("--squash");
    }
    if let Some(m) = message {
        cmd.arg("-m").arg(m);
    }
    cmd.arg(branch).current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git merge")
}

pub fn rebase(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    if let Err(e) = require_confirm(&v, "git_rebase") {
        return e;
    }
    let onto = v
        .get("onto")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let abort = v.get("abort").and_then(|x| x.as_bool()).unwrap_or(false);
    let cont = v.get("continue").and_then(|x| x.as_bool()).unwrap_or(false);

    let mut cmd = Command::new("git");
    cmd.arg("rebase");
    if abort {
        cmd.arg("--abort");
    } else if cont {
        cmd.arg("--continue");
    } else if let Some(target) = onto {
        cmd.arg(target);
    } else {
        return "错误：rebase 需要 onto 参数，或 abort=true / continue=true".to_string();
    }
    cmd.current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git rebase")
}

pub fn stash(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let action = v
        .get("action")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .unwrap_or("push");
    let message = v
        .get("message")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut cmd = Command::new("git");
    match action {
        "push" | "save" => {
            cmd.arg("stash").arg("push");
            if let Some(m) = message {
                cmd.arg("-m").arg(m);
            }
        }
        "pop" => {
            cmd.arg("stash").arg("pop");
        }
        "apply" => {
            cmd.arg("stash").arg("apply");
        }
        "list" => {
            cmd.arg("stash").arg("list");
        }
        "drop" => {
            cmd.arg("stash").arg("drop");
        }
        "clear" => {
            if let Err(e) = require_confirm(&v, "git_stash clear") {
                return e;
            }
            cmd.arg("stash").arg("clear");
        }
        _ => return format!("错误：不支持的 stash action: {}", action),
    }
    cmd.current_dir(working_dir);
    run_and_format(cmd, max_output_len, &format!("git stash {}", action))
}

pub fn tag(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    let action = v
        .get("action")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .unwrap_or("list");

    let mut cmd = Command::new("git");
    match action {
        "list" => {
            cmd.arg("tag").arg("-l");
            let pattern = v
                .get("pattern")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            if let Some(p) = pattern {
                cmd.arg(p);
            }
        }
        "create" => {
            let name = match v.get("name").and_then(|x| x.as_str()).map(str::trim) {
                Some(s) if !s.is_empty() => s,
                _ => return "错误：创建 tag 需要 name 参数".to_string(),
            };
            if name.contains("..") || name.starts_with('-') {
                return "错误：tag 名不合法".to_string();
            }
            let message = v
                .get("message")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            cmd.arg("tag");
            if let Some(m) = message {
                cmd.arg("-a").arg(name).arg("-m").arg(m);
            } else {
                cmd.arg(name);
            }
        }
        "delete" => {
            if let Err(e) = require_confirm(&v, "git_tag delete") {
                return e;
            }
            let name = match v.get("name").and_then(|x| x.as_str()).map(str::trim) {
                Some(s) if !s.is_empty() => s,
                _ => return "错误：删除 tag 需要 name 参数".to_string(),
            };
            cmd.arg("tag").arg("-d").arg(name);
        }
        _ => return format!("错误：不支持的 tag action: {}", action),
    }
    cmd.current_dir(working_dir);
    run_and_format(cmd, max_output_len, &format!("git tag {}", action))
}

pub fn reset(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    if let Err(e) = require_confirm(&v, "git_reset") {
        return e;
    }
    let mode = v
        .get("mode")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .unwrap_or("mixed");
    let target = v
        .get("target")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("HEAD");

    let flag = match mode {
        "soft" => "--soft",
        "mixed" => "--mixed",
        "hard" => "--hard",
        _ => return format!("错误：不支持的 reset mode: {}（仅 soft/mixed/hard）", mode),
    };
    let mut cmd = Command::new("git");
    cmd.arg("reset").arg(flag).arg(target);
    cmd.current_dir(working_dir);
    run_and_format(
        cmd,
        max_output_len,
        &format!("git reset {} {}", flag, target),
    )
}

pub fn cherry_pick(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    if let Err(e) = require_confirm(&v, "git_cherry_pick") {
        return e;
    }
    let abort = v.get("abort").and_then(|x| x.as_bool()).unwrap_or(false);
    let cont = v.get("continue").and_then(|x| x.as_bool()).unwrap_or(false);

    let mut cmd = Command::new("git");
    cmd.arg("cherry-pick");
    if abort {
        cmd.arg("--abort");
    } else if cont {
        cmd.arg("--continue");
    } else {
        let commits = match v.get("commits").and_then(|x| x.as_array()) {
            Some(arr) if !arr.is_empty() => arr
                .iter()
                .filter_map(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>(),
            _ => match v.get("commit").and_then(|x| x.as_str()).map(str::trim) {
                Some(s) if !s.is_empty() => vec![s],
                _ => return "错误：缺少 commit(s) 参数".to_string(),
            },
        };
        let no_commit = v
            .get("no_commit")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        if no_commit {
            cmd.arg("--no-commit");
        }
        for c in &commits {
            cmd.arg(*c);
        }
    }
    cmd.current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git cherry-pick")
}

pub fn revert(args_json: &str, max_output_len: usize, working_dir: &Path) -> String {
    let v = match parse_args(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = ensure_git_repo(working_dir) {
        return e;
    }
    if let Err(e) = require_confirm(&v, "git_revert") {
        return e;
    }
    let abort = v.get("abort").and_then(|x| x.as_bool()).unwrap_or(false);
    let cont = v.get("continue").and_then(|x| x.as_bool()).unwrap_or(false);

    let mut cmd = Command::new("git");
    cmd.arg("revert");
    if abort {
        cmd.arg("--abort");
    } else if cont {
        cmd.arg("--continue");
    } else {
        let commit = match require_string_field(&v, "commit") {
            Ok(s) => s,
            Err(e) => return e,
        };
        let no_commit = v
            .get("no_commit")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        if no_commit {
            cmd.arg("--no-commit");
        }
        cmd.arg(commit);
    }
    cmd.current_dir(working_dir);
    run_and_format(cmd, max_output_len, "git revert")
}

#[cfg(test)]
mod tests {
    use super::super::helpers::is_safe_rel_path;

    #[test]
    fn test_is_safe_rel_path() {
        assert!(is_safe_rel_path("src/main.rs"));
        assert!(!is_safe_rel_path("/etc/passwd"));
        assert!(!is_safe_rel_path("../x"));
        assert!(!is_safe_rel_path(""));
    }
}
