use std::path::Path;

use super::common::{
    gh_allowed, parse_gh_tool_args, push_list_tail_argv, push_repo_argv, push_view_tail_argv,
    run_gh_vec,
};

fn parse_view_number(v: &serde_json::Value, max: u64, err_msg: &str) -> Result<String, String> {
    match v.get("number").and_then(|x| x.as_u64()) {
        Some(n) if n > 0 && n <= max => Ok(n.to_string()),
        _ => Err(err_msg.to_string()),
    }
}

/// `gh pr list`
pub fn gh_pr_list(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v = match parse_gh_tool_args(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let mut argv = vec!["pr".into(), "list".into()];
    if let Err(e) = push_repo_argv(&mut argv, &v) {
        return e;
    }
    if let Err(e) = push_list_tail_argv(&mut argv, &v, true) {
        return e;
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

/// `gh pr view <n>`
pub fn gh_pr_view(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v = match parse_gh_tool_args(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let num = match parse_view_number(
        &v,
        999_999,
        "错误：缺少或非法 number（须为 1～999999 的正整数）",
    ) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let mut argv = vec!["pr".into(), "view".into(), num];
    if let Err(e) = push_view_tail_argv(&mut argv, &v) {
        return e;
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

/// `gh issue list`
pub fn gh_issue_list(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v = match parse_gh_tool_args(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let mut argv = vec!["issue".into(), "list".into()];
    if let Err(e) = push_repo_argv(&mut argv, &v) {
        return e;
    }
    if let Err(e) = push_list_tail_argv(&mut argv, &v, false) {
        return e;
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

/// `gh issue view <n>`
pub fn gh_issue_view(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v = match parse_gh_tool_args(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let num = match parse_view_number(&v, 9_999_999, "错误：缺少或非法 number（须为正整数）")
    {
        Ok(n) => n,
        Err(e) => return e,
    };
    let mut argv = vec!["issue".into(), "view".into(), num];
    if let Err(e) = push_view_tail_argv(&mut argv, &v) {
        return e;
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}
