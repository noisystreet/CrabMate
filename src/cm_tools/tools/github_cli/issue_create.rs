//! `gh issue create`（写远端 Issue）。

use crate::cm_tools::tool_result::ToolError;
use super::common::GhCtx;
use serde_json::Value as JsonValue;

use super::common::{
    gh_allowed, push_bool_flag, push_extra_args_from_json, push_repo_arg, push_trimmed_string_flag,
    run_gh_vec, validate_pr_body, validate_pr_title, write_workspace_temp_markdown,
};

fn gh_issue_create_build_argv(
    v: &JsonValue,
    title: &str,
    body_path: String,
) -> Result<Vec<String>, ToolError> {
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
pub fn gh_issue_create_try(
    args_json: &str,
    ctx: &GhCtx<'_>,
) -> Result<String, ToolError> {
    gh_allowed(ctx.allowed_commands)?;
    let v = crate::cm_tools::tools::parse_args_json(args_json)
        .map_err(ToolError::invalid_args)?;
    let title = match v.get("title").and_then(|x| x.as_str()) {
        Some(s) => s,
        None => return Err(ToolError::invalid_args("错误：缺少 title".to_string())),
    };
    validate_pr_title(title)?;
    let body = v.get("body").and_then(|x| x.as_str()).unwrap_or("");
    validate_pr_body(body)?;

    let (dir, body_path) = write_workspace_temp_markdown(
        ctx.working_dir,
        "crabmate_issue_body.md",
        body.as_bytes(),
        "Issue 正文",
    )?;
    let argv = gh_issue_create_build_argv(&v, title, body_path)?;
    let out = run_gh_vec(argv, ctx);
    drop(dir);
    out
}
