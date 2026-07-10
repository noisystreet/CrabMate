//! `gh pr merge` / `gh pr review` / `gh pr comment`（写远端 PR 状态）。

use std::path::Path;

use super::common::{gh_allowed, run_gh_vec, validate_extra_args, validate_pr_body, validate_repo};

fn parse_optional_pr_number(v: &serde_json::Value) -> Result<Option<String>, String> {
    match v.get("number") {
        None => Ok(None),
        Some(n) => {
            let num = n
                .as_u64()
                .ok_or_else(|| "错误：number 须为正整数".to_string())?;
            if num == 0 || num > 999_999 {
                return Err("错误：number 须为 1～999999 的正整数或省略".to_string());
            }
            Ok(Some(num.to_string()))
        }
    }
}

fn push_repo(v: &serde_json::Value, argv: &mut Vec<String>) -> Result<(), String> {
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        validate_repo(r)?;
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    Ok(())
}

fn push_extra(v: &serde_json::Value, argv: &mut Vec<String>) -> Result<(), String> {
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        let extra: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        validate_extra_args(&extra)?;
        argv.extend(extra);
    }
    Ok(())
}

/// `gh pr merge`（写远端：合并 PR）
pub fn gh_pr_merge(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let mut argv = vec!["pr".into(), "merge".into()];
    if let Ok(Some(num)) = parse_optional_pr_number(&v) {
        argv.push(num);
    }
    if let Err(e) = push_repo(&v, &mut argv) {
        return e;
    }
    let method = v
        .get("merge_method")
        .and_then(|x| x.as_str())
        .unwrap_or("merge")
        .trim()
        .to_ascii_lowercase();
    match method.as_str() {
        "merge" => argv.push("--merge".into()),
        "squash" => argv.push("--squash".into()),
        "rebase" => argv.push("--rebase".into()),
        _ => return "错误：merge_method 须为 merge、squash 或 rebase".to_string(),
    }
    if v.get("auto").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--auto".into());
    }
    if v.get("delete_branch").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--delete-branch".into());
    }
    if v.get("admin").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--admin".into());
    }
    if let Err(e) = push_extra(&v, &mut argv) {
        return e;
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

/// `gh pr review`（写远端：审批 / 请求修改 / 评论）
pub fn gh_pr_review(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let event = match v.get("event").and_then(|x| x.as_str()) {
        Some(s) => s.trim().to_ascii_lowercase(),
        None => return "错误：缺少 event（approve / request-changes / comment）".to_string(),
    };
    let mut argv = vec!["pr".into(), "review".into()];
    if let Ok(Some(num)) = parse_optional_pr_number(&v) {
        argv.push(num);
    }
    if let Err(e) = push_repo(&v, &mut argv) {
        return e;
    }
    match event.as_str() {
        "approve" => argv.push("--approve".into()),
        "request-changes" | "request_changes" => argv.push("--request-changes".into()),
        "comment" => argv.push("--comment".into()),
        _ => return "错误：event 须为 approve、request-changes 或 comment".to_string(),
    }
    if let Some(b) = v.get("body").and_then(|x| x.as_str()) {
        if let Err(e) = validate_pr_body(b) {
            return e;
        }
        if !b.trim().is_empty() {
            argv.push("--body".into());
            argv.push(b.trim().to_string());
        }
    } else if matches!(
        event.as_str(),
        "comment" | "request-changes" | "request_changes"
    ) {
        return "错误：comment / request-changes 须提供 body".to_string();
    }
    if let Err(e) = push_extra(&v, &mut argv) {
        return e;
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

/// `gh pr comment`（写远端：在 PR 上评论）
pub fn gh_pr_comment(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let body = match v.get("body").and_then(|x| x.as_str()) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return "错误：缺少 body".to_string(),
    };
    if let Err(e) = validate_pr_body(body) {
        return e;
    }
    let mut argv = vec!["pr".into(), "comment".into()];
    if let Ok(Some(num)) = parse_optional_pr_number(&v) {
        argv.push(num);
    }
    if let Err(e) = push_repo(&v, &mut argv) {
        return e;
    }
    argv.push("--body".into());
    argv.push(body.trim().to_string());
    if let Err(e) = push_extra(&v, &mut argv) {
        return e;
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}
