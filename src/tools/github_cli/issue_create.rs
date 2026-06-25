//! `gh issue create`（写远端 Issue）。

use std::path::Path;

use serde_json::Value as JsonValue;

use super::common::{
    gh_allowed, push_bool_flag, push_extra_args_from_json, push_repo_arg, push_trimmed_string_flag,
    run_gh_vec, validate_pr_body, validate_pr_title, write_workspace_temp_markdown,
};

fn gh_issue_create_build_argv(
    v: &JsonValue,
    title: &str,
    body_path: String,
) -> Result<Vec<String>, String> {
    let mut argv = vec![
        "issue".into(),
        "create".into(),
        "--title".into(),
        title.trim().to_string(),
        "--body-file".into(),
        body_path,
    ];
    push_repo_arg(v, &mut argv)?;
    if let Some(arr) = v.get("labels").and_then(|x| x.as_array()) {
        for label in arr.iter().filter_map(|x| x.as_str()) {
            let t = label.trim();
            if !t.is_empty() {
                argv.push("--label".into());
                argv.push(t.to_string());
            }
        }
    }
    push_trimmed_string_flag(v, "assignee", "--assignee", &mut argv);
    push_bool_flag(v, "web", "--web", &mut argv);
    push_extra_args_from_json(v, &mut argv)?;
    Ok(argv)
}

/// `gh issue create`（正文经工作区临时 `--body-file`）
pub fn gh_issue_create(
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
    let title = match v.get("title").and_then(|x| x.as_str()) {
        Some(s) => s,
        None => return "错误：缺少 title".to_string(),
    };
    if let Err(e) = validate_pr_title(title) {
        return e;
    }
    let body = v.get("body").and_then(|x| x.as_str()).unwrap_or("");
    if let Err(e) = validate_pr_body(body) {
        return e;
    }

    let (dir, body_path) = match write_workspace_temp_markdown(
        working_dir,
        "crabmate_issue_body.md",
        body.as_bytes(),
        "Issue 正文",
    ) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let argv = match gh_issue_create_build_argv(&v, title, body_path) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let out = run_gh_vec(argv, max_output_len, allowed_commands, working_dir);
    drop(dir);
    out
}
