use std::path::Path;

use serde_json::Value as JsonValue;

use super::common::{
    clamp_limit, command_formatted_exit_code, extract_stderr_from_formatted, gh_allowed,
    join_json_fields, push_bool_flag, push_extra_args_from_json, push_repo_arg,
    push_trimmed_string_flag, run_gh_vec, validate_extra_args, validate_pr_body,
    validate_pr_ref_token, validate_pr_title, validate_repo, write_workspace_temp_markdown,
};
use super::pr_body::build_pr_body_draft;
use super::run_ci::{
    PR_CHECKS_STRUCTURED_JSON_FIELDS, finalize_structured_pr_checks, gh_pr_checks_rejects_json_flag,
};

/// `gh run list`
pub fn gh_run_list(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v = match crate::cm_tools::tools::parse_args_json(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let mut argv = vec!["run".into(), "list".into()];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if let Err(e) = validate_repo(r) {
            return e;
        }
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    let lim = clamp_limit(v.get("limit").and_then(|x| x.as_u64()).map(|u| u as u32));
    argv.push("--limit".into());
    argv.push(lim.to_string());
    if let Some(arr) = v.get("fields").and_then(|x| x.as_array()) {
        let fields: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        match join_json_fields(&fields) {
            Ok(j) => {
                argv.push("--json".into());
                argv.push(j);
            }
            Err(e) => return e,
        }
    }
    if v.get("web").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--web".into());
    }
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        let extra: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if let Err(e) = validate_extra_args(&extra) {
            return e;
        }
        argv.extend(extra);
    }
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

fn gh_pr_diff_argv(v: &JsonValue) -> Result<Vec<String>, String> {
    let num = match v.get("number").and_then(|x| x.as_u64()) {
        Some(n) if n > 0 && n <= 999_999 => n.to_string(),
        _ => return Err("错误：缺少或非法 number".to_string()),
    };
    let mut argv = vec!["pr".into(), "diff".into(), num];
    push_repo_arg(v, &mut argv)?;
    push_bool_flag(v, "patch", "--patch", &mut argv);
    push_extra_args_from_json(v, &mut argv)?;
    Ok(argv)
}

/// `gh pr diff`（只读）
pub fn gh_pr_diff(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v = match crate::cm_tools::tools::parse_args_json(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    match gh_pr_diff_argv(&v) {
        Ok(argv) => run_gh_vec(argv, max_output_len, allowed_commands, working_dir),
        Err(e) => e,
    }
}

fn build_pr_checks_argv(v: &JsonValue, with_structured_json: bool) -> Result<Vec<String>, String> {
    let mut argv = vec!["pr".into(), "checks".into()];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        validate_repo(r)?;
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    if let Some(n) = v.get("number").and_then(|x| x.as_u64()) {
        if n == 0 || n > 999_999 {
            return Err("错误：number 须为 1～999999 的正整数或省略".to_string());
        }
        argv.push(n.to_string());
    }
    if with_structured_json {
        argv.push("--json".into());
        argv.push(PR_CHECKS_STRUCTURED_JSON_FIELDS.into());
    }
    if let Some(arr) = v.get("extra_args").and_then(|x| x.as_array()) {
        let extra: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        validate_extra_args(&extra)?;
        argv.extend(extra);
    }
    Ok(argv)
}

fn gh_pr_checks_table_fallback(
    v: &JsonValue,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    let argv = match build_pr_checks_argv(v, false) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let table = run_gh_vec(argv, max_output_len, allowed_commands, working_dir);
    format!(
        "{}\n\n---\n提示：本机 `gh` 不支持 `pr checks --json`（需 GitHub CLI ≥ 2.50）。已回退为表格输出；请升级 `gh` 后再用 `structured: true`。\n",
        table.trim_end()
    )
}

/// `gh pr checks`（只读）：CI 检查状态；省略 `number` 时使用当前分支关联的 PR（与 `gh` 默认一致）。
pub fn gh_pr_checks(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let v = match crate::cm_tools::tools::parse_args_json(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let structured = v.get("structured").and_then(|x| x.as_bool()) == Some(true);
    let argv = match build_pr_checks_argv(&v, structured) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let out = run_gh_vec(argv, max_output_len, allowed_commands, working_dir);
    if !structured {
        return out;
    }
    if gh_pr_checks_rejects_json_flag(&out) {
        return gh_pr_checks_table_fallback(&v, max_output_len, allowed_commands, working_dir);
    }
    finalize_structured_pr_checks(out)
}

fn resolve_pr_create_body(v: &JsonValue, working_dir: &Path) -> Result<String, String> {
    let auto_body = v.get("auto_body").and_then(|x| x.as_bool()).unwrap_or(true);
    match v.get("body").and_then(|x| x.as_str()) {
        Some(s) if !s.trim().is_empty() => Ok(s.to_string()),
        _ if auto_body => {
            let base = v.get("base").and_then(|x| x.as_str());
            build_pr_body_draft(working_dir, base, 30, true, true)
        }
        _ => Ok(String::new()),
    }
}

fn gh_pr_create_validate_repo_base_head(v: &JsonValue) -> Result<(), String> {
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        validate_repo(r)?;
    }
    if let Some(b) = v.get("base").and_then(|x| x.as_str()) {
        validate_pr_ref_token(b)?;
    }
    if let Some(h) = v.get("head").and_then(|x| x.as_str()) {
        validate_pr_ref_token(h)?;
    }
    Ok(())
}

fn gh_pr_create_build_argv(
    v: &JsonValue,
    title: &str,
    body_path_str: String,
) -> Result<Vec<String>, String> {
    let mut argv = vec![
        "pr".into(),
        "create".into(),
        "--title".into(),
        title.trim().to_string(),
        "--body-file".into(),
        body_path_str,
    ];
    push_repo_arg(v, &mut argv)?;
    push_trimmed_string_flag(v, "base", "--base", &mut argv);
    push_trimmed_string_flag(v, "head", "--head", &mut argv);
    push_bool_flag(v, "draft", "--draft", &mut argv);
    push_bool_flag(v, "web", "--web", &mut argv);
    push_extra_args_from_json(v, &mut argv)?;
    Ok(argv)
}

/// `gh pr create` 失败时对常见 GraphQL / gh 错误归类，追加可操作提示（LLM 可据此纠正后重试）。
/// 退出码 0 时原样返回；无法识别的失败也原样返回（避免误报）。
fn annotate_gh_pr_create_failure(formatted: String) -> String {
    if command_formatted_exit_code(&formatted) == Some(0) {
        return formatted;
    }
    let stderr = extract_stderr_from_formatted(&formatted).to_ascii_lowercase();
    let hint = if stderr.contains("no commits between")
        || stderr.contains("head sha can't be blank")
        || stderr.contains("base sha can't be blank")
    {
        "head 分支与 base 分支之间没有提交差异，或 head 分支尚未推送到远端。请先用 `git push -u origin <head>` 推送包含至少一个提交的分支，并确认 head/base 分支名拼写无误后再重试。"
    } else if stderr.contains("base ref must be a branch") {
        "`base` 必须是仓库中已存在的分支（如 main/master）。请确认 base 分支名拼写后再重试。"
    } else if stderr.contains("a pull request already exists") {
        "该 head 分支已有关联 PR。请改用 `gh_pr_view` 查看现有 PR，而非重复创建。"
    } else if stderr.contains("repository not found")
        || stderr.contains("could not resolve to a repository")
    {
        "仓库不存在或当前 `gh` 身份无权访问。请确认 `repo` 参数（owner/repo）拼写与 `gh auth status` 的权限。"
    } else {
        return formatted;
    };
    format!("{}\n\n---\n提示：{}\n", formatted.trim_end(), hint)
}

fn gh_pr_create_inputs(
    args_json: &str,
    working_dir: &Path,
) -> Result<(JsonValue, String, String), String> {
    let v = crate::cm_tools::tools::parse_args_json(args_json)?;
    let title = v
        .get("title")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "错误：缺少 title".to_string())?
        .to_string();
    validate_pr_title(&title)?;
    let body_str = resolve_pr_create_body(&v, working_dir)?;
    validate_pr_body(&body_str)?;
    gh_pr_create_validate_repo_base_head(&v)?;
    Ok((v, title, body_str))
}

fn write_pr_create_body_temp(
    working_dir: &Path,
    body: &[u8],
) -> Result<(tempfile::TempDir, String), String> {
    // 与历史文案对齐：路径非 UTF-8 时固定为「临时文件路径非 UTF-8」（不用带 label 的通用句）。
    match write_workspace_temp_markdown(working_dir, "crabmate_pr_body.md", body, "PR 正文") {
        Ok(x) => Ok(x),
        Err(e) if e.contains("临时文件路径非 UTF-8") => {
            Err("错误：临时文件路径非 UTF-8".to_string())
        }
        Err(e) => Err(e),
    }
}

/// `gh pr create`（在远端创建 PR；**写操作**）。`title` + `body` 经工作区内临时文件以 `--body-file` 传入，避免 shell 转义问题。
pub fn gh_pr_create(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    if let Err(e) = gh_allowed(allowed_commands) {
        return e;
    }
    let (v, title, body_str) = match gh_pr_create_inputs(args_json, working_dir) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let (dir, body_path_str) = match write_pr_create_body_temp(working_dir, body_str.as_bytes()) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let argv = match gh_pr_create_build_argv(&v, &title, body_path_str) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let out = run_gh_vec(argv, max_output_len, allowed_commands, working_dir);
    drop(dir);
    annotate_gh_pr_create_failure(out)
}

#[cfg(test)]
mod tests {
    use super::annotate_gh_pr_create_failure;

    fn gh_create_err(stderr: &str) -> String {
        format!(
            "命令：gh pr create --title t --body-file /tmp/b.md --base main --head feat/x\n退出码：1\n标准错误：\n{stderr}"
        )
    }

    #[test]
    fn annotate_no_commits_between_suggests_push() {
        let raw = gh_create_err(
            "pull request create failed: GraphQL: Head sha can't be blank, Base sha can't be blank, \
             No commits between main and feat/avx-example, Base ref must be a branch (createPullRequest)",
        );
        let out = annotate_gh_pr_create_failure(raw);
        assert!(out.contains("push -u origin"), "{}", out);
        assert!(out.contains("提示"), "{}", out);
    }

    #[test]
    fn annotate_base_ref_must_be_branch() {
        let raw = gh_create_err("GraphQL: Base ref must be a branch (createPullRequest)");
        let out = annotate_gh_pr_create_failure(raw);
        assert!(out.contains("必须") && out.contains("base"), "{}", out);
    }

    #[test]
    fn annotate_pr_already_exists_points_to_view() {
        let raw = gh_create_err("a pull request already exists for feat/x (createPullRequest)");
        let out = annotate_gh_pr_create_failure(raw);
        assert!(out.contains("gh_pr_view"), "{}", out);
    }

    #[test]
    fn annotate_repo_not_found() {
        let raw =
            gh_create_err("GraphQL: Could not resolve to a Repository with the name 'no/such'");
        let out = annotate_gh_pr_create_failure(raw);
        assert!(out.contains("仓库"), "{}", out);
    }

    #[test]
    fn annotate_bare_not_found_is_left_untouched() {
        // 裸 "not found" 不命中任何分类，避免误判为「仓库不存在」。
        let raw = gh_create_err("GraphQL: Some other thing not found (createPullRequest)");
        let out = annotate_gh_pr_create_failure(raw.clone());
        assert_eq!(out, raw);
    }

    #[test]
    fn annotate_keeps_success_output_untouched() {
        let raw =
            "命令：gh pr create --title t\n退出码：0\n标准输出：\nhttps://github.com/o/r/pull/1\n";
        let out = annotate_gh_pr_create_failure(raw.to_string());
        assert_eq!(out, raw);
    }

    #[test]
    fn annotate_leaves_unrecognized_failure_untouched() {
        let raw = gh_create_err("some unrelated error");
        let out = annotate_gh_pr_create_failure(raw.clone());
        assert_eq!(out, raw);
    }
}
