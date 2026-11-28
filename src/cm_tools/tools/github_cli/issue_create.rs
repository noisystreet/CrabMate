//! `gh issue create`（写远端 Issue）。

use super::common::GhCtx;
use crate::cm_tool_result::ToolError;
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
pub fn gh_issue_create_try(args_json: &str, ctx: &GhCtx<'_>) -> Result<String, ToolError> {
    gh_allowed(ctx.allowed_commands)?;
    let v = crate::cm_tools::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 钉住 `GhIssueCreateArgs` 字段 doc 里写明的 flag 映射。
    /// 这个文件此前没有测试，而字段 doc 是模型唯一能看到的语义。
    #[test]
    fn argv_maps_documented_fields_to_flags() {
        let v = json!({
            "title": "t",
            "repo": "owner/repo",
            "labels": ["bug", "   ", "docs"],
            "assignee": "  octocat  ",
            "web": true,
            "extra_args": ["--yes"]
        });
        let argv =
            gh_issue_create_build_argv(&v, "  t  ", "body.md".into()).expect("argv 应可构造");
        assert_eq!(
            argv,
            [
                "issue",
                "create",
                "--title",
                "t",
                "--body-file",
                "body.md",
                "-R",
                "owner/repo",
                "--label",
                "bug",
                // 空白标签被跳过，不留 `--label ""`
                "--label",
                "docs",
                "--assignee",
                "octocat",
                "--web",
                "--yes",
            ]
        );
    }
}
