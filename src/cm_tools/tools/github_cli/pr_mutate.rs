//! `gh pr merge` / `gh pr review` / `gh pr comment`（写远端 PR 状态）。

use crate::cm_tools::tool_result::ToolError;
use super::common::GhCtx;
use std::path::Path;

use super::common::{
    gh_allowed, run_gh_vec, validate_extra_args, validate_pr_body, validate_pr_ref_token,
    validate_pr_title, validate_repo, write_workspace_temp_markdown,
};

fn parse_optional_pr_number(v: &serde_json::Value) -> Result<Option<String>, ToolError> {
    match v.get("number") {
        None => Ok(None),
        Some(n) => {
            let num = n
                .as_u64()
                .ok_or_else(|| ToolError::invalid_args("错误：number 须为正整数".to_string()))?;
            if num == 0 || num > 999_999 {
                return Err(ToolError::invalid_args("错误：number 须为 1～999999 的正整数或省略".to_string()));
            }
            Ok(Some(num.to_string()))
        }
    }
}

fn push_repo(v: &serde_json::Value, argv: &mut Vec<String>) -> Result<(), ToolError> {
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        validate_repo(r)?;
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    Ok(())
}

fn push_extra(v: &serde_json::Value, argv: &mut Vec<String>) -> Result<(), ToolError> {
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
pub fn gh_pr_merge_try(
    args_json: &str,
    ctx: &GhCtx<'_>,
) -> Result<String, ToolError> {
    gh_allowed(ctx.allowed_commands)?;
    let v = crate::cm_tools::tools::parse_args_json(args_json)
        .map_err(ToolError::invalid_args)?;
    let mut argv = vec!["pr".into(), "merge".into()];
    if let Ok(Some(num)) = parse_optional_pr_number(&v) {
        argv.push(num);
    }
    push_repo(&v, &mut argv)?;
    let method = v
        .get("merge_method")
        .and_then(|x| x.as_str())
        .unwrap_or("rebase")
        .trim()
        .to_ascii_lowercase();
    match method.as_str() {
        "merge" => argv.push("--merge".into()),
        "squash" => argv.push("--squash".into()),
        "rebase" => argv.push("--rebase".into()),
        _ => return Err(ToolError::invalid_args("错误：merge_method 须为 merge、squash 或 rebase".to_string())),
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
    push_extra(&v, &mut argv)?;
    run_gh_vec(argv, ctx)
}

fn gh_pr_review_argv(v: &serde_json::Value) -> Result<Vec<String>, ToolError> {
    let event = match v.get("event").and_then(|x| x.as_str()) {
        Some(s) => s.trim().to_ascii_lowercase(),
        None => return Err(ToolError::invalid_args("错误：缺少 event（approve / request-changes / comment）".to_string())),
    };
    let mut argv = vec!["pr".into(), "review".into()];
    if let Ok(Some(num)) = parse_optional_pr_number(v) {
        argv.push(num);
    }
    push_repo(v, &mut argv)?;
    match event.as_str() {
        "approve" => argv.push("--approve".into()),
        "request-changes" | "request_changes" => argv.push("--request-changes".into()),
        "comment" => argv.push("--comment".into()),
        _ => return Err(ToolError::invalid_args("错误：event 须为 approve、request-changes 或 comment".to_string())),
    }
    if let Some(b) = v.get("body").and_then(|x| x.as_str()) {
        validate_pr_body(b)?;
        if !b.trim().is_empty() {
            argv.push("--body".into());
            argv.push(b.trim().to_string());
        }
    } else if matches!(
        event.as_str(),
        "comment" | "request-changes" | "request_changes"
    ) {
        return Err(ToolError::invalid_args("错误：comment / request-changes 须提供 body".to_string()));
    }
    push_extra(v, &mut argv)?;
    Ok(argv)
}

/// `gh pr review`（写远端：审批 / 请求修改 / 评论）
pub fn gh_pr_review_try(
    args_json: &str,
    ctx: &GhCtx<'_>,
) -> Result<String, ToolError> {
    gh_allowed(ctx.allowed_commands)?;
    let v = crate::cm_tools::tools::parse_args_json(args_json)
        .map_err(ToolError::invalid_args)?;
    match gh_pr_review_argv(&v) {
        Ok(argv) => run_gh_vec(argv, ctx),
        Err(e) => Err(e),
    }
}

/// `gh pr comment`（写远端：在 PR 上评论）
pub fn gh_pr_comment_try(
    args_json: &str,
    ctx: &GhCtx<'_>,
) -> Result<String, ToolError> {
    gh_allowed(ctx.allowed_commands)?;
    let v = crate::cm_tools::tools::parse_args_json(args_json)
        .map_err(ToolError::invalid_args)?;
    let body = match v.get("body").and_then(|x| x.as_str()) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return Err(ToolError::invalid_args("错误：缺少 body".to_string())),
    };
    validate_pr_body(body)?;
    let mut argv = vec!["pr".into(), "comment".into()];
    if let Ok(Some(num)) = parse_optional_pr_number(&v) {
        argv.push(num);
    }
    push_repo(&v, &mut argv)?;
    argv.push("--body".into());
    argv.push(body.trim().to_string());
    push_extra(&v, &mut argv)?;
    run_gh_vec(argv, ctx)
}

/// `gh pr edit`（写远端：编辑 PR 标题 / 正文 / 标签 / reviewer / assignee / 基线分支 / 草稿状态）。
pub fn gh_pr_edit_try(
    args_json: &str,
    ctx: &GhCtx<'_>,
) -> Result<String, ToolError> {
    gh_allowed(ctx.allowed_commands)?;
    let v = crate::cm_tools::tools::parse_args_json(args_json)
        .map_err(ToolError::invalid_args)?;

    // 若提供 body，先写入工作区内临时文件（与 `gh_pr_create` 一致），
    // 再统一构建 argv，使 body 可与标签 / base / draft 等编辑项组合，避免提前返回丢弃其余编辑项。
    let mut body_opt: Option<(tempfile::TempDir, String)> = None;
    if let Some(b) = v.get("body").and_then(|x| x.as_str()) {
        validate_pr_body(b)?;
        let (dir, body_path) = write_workspace_temp_markdown(
            ctx.working_dir,
            "crabmate_pr_body.md",
            b.as_bytes(),
            "PR 正文",
        )?;
        body_opt = Some((dir, body_path.clone()));
    }
    let argv = build_gh_pr_edit_argv(&v, body_opt.as_ref().map(|(_, p)| p.clone()))?;
    let out = run_gh_vec(argv, ctx);
    if let Some((dir, _)) = body_opt {
        drop(dir);
    }
    out
}

/// 构建 `gh pr edit` 的 argv。`body_path_opt` 为调用方已写入的临时正文文件路径
/// （`None` 表示本次不编辑正文）；校验所有编辑项至少提供其一。
fn build_gh_pr_edit_argv(
    v: &serde_json::Value,
    body_path_opt: Option<String>,
) -> Result<Vec<String>, ToolError> {
    let mut argv = vec!["pr".into(), "edit".into()];
    if let Ok(Some(num)) = parse_optional_pr_number(v) {
        argv.push(num);
    }
    push_repo(v, &mut argv)?;

    let mut has_edit = false;
    has_edit |= push_scalar_edits(v, body_path_opt, &mut argv)?;
    has_edit |= push_multi_edit_items(v, &mut argv)?;
    if !has_edit {
        return Err(ToolError::invalid_args("错误：gh pr edit 至少需要 title / body / add_label / remove_label / add_reviewer / remove_reviewer / add_assignee / remove_assignee / base / draft / undraft 之一"
                .to_string()));
    }
    push_extra(v, &mut argv)?;
    Ok(argv)
}

/// 处理标量编辑项：title、body、base、draft/undraft。返回是否产生了任何编辑。
fn push_scalar_edits(
    v: &serde_json::Value,
    body_path_opt: Option<String>,
    argv: &mut Vec<String>,
) -> Result<bool, ToolError> {
    let mut has_edit = false;
    if let Some(t) = v.get("title").and_then(|x| x.as_str()) {
        validate_pr_title(t)?;
        let tt = t.trim();
        if !tt.is_empty() {
            argv.push("--title".into());
            argv.push(tt.to_string());
            has_edit = true;
        }
    }
    if let Some(path) = body_path_opt {
        argv.push("--body-file".into());
        argv.push(path);
        has_edit = true;
    }
    if let Some(b) = v.get("base").and_then(|x| x.as_str()) {
        validate_pr_ref_token(b)?;
        let bb = b.trim();
        if !bb.is_empty() {
            argv.push("--base".into());
            argv.push(bb.to_string());
            has_edit = true;
        }
    }
    if v.get("draft").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--draft".into());
        has_edit = true;
    }
    if v.get("undraft").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--undraft".into());
        has_edit = true;
    }
    Ok(has_edit)
}

/// 处理可重复的标签 / reviewer / assignee 编辑项。返回是否产生了任何编辑。
fn push_multi_edit_items(v: &serde_json::Value, argv: &mut Vec<String>) -> Result<bool, ToolError> {
    let mut has_edit = false;
    for (key, flag) in [
        ("add_label", "--add-label"),
        ("remove_label", "--remove-label"),
        ("add_reviewer", "--add-reviewer"),
        ("remove_reviewer", "--remove-reviewer"),
        ("add_assignee", "--add-assignee"),
        ("remove_assignee", "--remove-assignee"),
    ] {
        if let Some(arr) = v.get(key).and_then(|x| x.as_array()) {
            for item in arr.iter().filter_map(|x| x.as_str()) {
                let t = item.trim();
                if t.is_empty() {
                    continue;
                }
                if !is_safe_token_item(t) {
                    return Err(ToolError::invalid_args(format!(
                        "错误：{key} 中含非法值 {t:?}（不得含 \"..\" 或以 \"/\" 开头）"
                    )));
                }
                argv.push(flag.into());
                argv.push(t.to_string());
                has_edit = true;
            }
        }
    }
    Ok(has_edit)
}

/// 校验 `gh pr edit` 多值参数项（标签 / reviewer / assignee 等）：不允许 `..` 或以 `/` 开头。
fn is_safe_token_item(s: &str) -> bool {
    !s.contains("..") && !s.starts_with('/')
}

#[cfg(test)]
mod tests {
    use super::{build_gh_pr_edit_argv, is_safe_token_item};
    use crate::cm_tools::tools::github_cli::GhCtx;

    #[test]
    fn gh_pr_edit_requires_gh_in_allowlist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = super::gh_pr_edit_try(r#"{"title":"t"}"#, &GhCtx::new(4096, &[], dir.path()))
            .expect_err("白名单无 gh 时必须失败");
        assert_eq!(err.code, "command_not_allowed");
        assert!(err.message.contains("未包含 gh"), "{}", err.message);
    }

    #[test]
    fn build_argv_title_and_body() {
        let v = serde_json::json!({"title": " 新标题 ", "body": "正文"});
        let argv = build_gh_pr_edit_argv(&v, Some("tmp/body.md".into())).expect("ok");
        assert_eq!(argv[0], "pr");
        assert_eq!(argv[1], "edit");
        assert!(argv.iter().any(|a| a == "--title"), "{argv:?}");
        assert!(argv.iter().any(|a| a == "新标题"), "{argv:?}");
        assert!(argv.iter().any(|a| a == "--body-file"), "{argv:?}");
        assert!(argv.iter().any(|a| a == "tmp/body.md"), "{argv:?}");
    }

    #[test]
    fn build_argv_body_combines_with_labels_and_base() {
        let v = serde_json::json!({
            "body": "正文",
            "add_label": ["bug", "enh"],
            "base": "main",
            "draft": true
        });
        let argv = build_gh_pr_edit_argv(&v, Some("tmp/body.md".into())).expect("ok");
        assert!(argv.iter().any(|a| a == "--body-file"), "{argv:?}");
        assert!(argv.iter().any(|a| a == "--add-label"), "{argv:?}");
        assert!(argv.iter().any(|a| a == "bug"), "{argv:?}");
        assert!(
            argv.iter().any(|a| a == "--base") && argv.iter().any(|a| a == "main"),
            "{argv:?}"
        );
        assert!(argv.iter().any(|a| a == "--draft"), "{argv:?}");
    }

    #[test]
    fn build_argv_rejects_no_edit() {
        let v = serde_json::json!({});
        let err = build_gh_pr_edit_argv(&v, None).expect_err("should err");
        assert!(err.message.contains("至少需要"), "{}", err);
    }

    #[test]
    fn build_argv_rejects_unsafe_label() {
        let v = serde_json::json!({"add_label": ["../x"]});
        let err = build_gh_pr_edit_argv(&v, None).expect_err("should err");
        assert!(err.message.contains("非法值"), "{}", err);
    }

    #[test]
    fn build_argv_rejects_bad_title() {
        let v = serde_json::json!({"title": "a\nb"});
        let err = build_gh_pr_edit_argv(&v, None).expect_err("should err");
        assert!(err.message.contains("title"), "{}", err);
    }

    #[test]
    fn build_argv_rejects_bad_base() {
        let v = serde_json::json!({"base": "main..x"});
        let err = build_gh_pr_edit_argv(&v, None).expect_err("should err");
        assert!(err.message.contains("base"), "{}", err);
    }

    #[test]
    fn is_safe_token_item_cases() {
        assert!(!is_safe_token_item("../x"));
        assert!(!is_safe_token_item("/abs"));
        assert!(is_safe_token_item("bug"));
        assert!(is_safe_token_item("feat/x"));
    }
}
