use std::path::Path;

use serde_json::Value as JsonValue;

use super::common::{
    clamp_limit, gh_allowed, join_json_fields, push_bool_flag, push_extra_args_from_json,
    push_repo_arg, push_trimmed_string_flag, run_gh_vec, validate_extra_args, validate_pr_body,
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
    let v = match crate::tools::parse_args_json(args_json) {
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
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let num = match v.get("number").and_then(|x| x.as_u64()) {
        Some(n) if n > 0 && n <= 999_999 => n.to_string(),
        _ => return "错误：缺少或非法 number".to_string(),
    };
    let mut argv = vec!["pr".into(), "diff".into(), num];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if let Err(e) = validate_repo(r) {
            return e;
        }
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    if v.get("patch").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--patch".into());
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
    let v = match crate::tools::parse_args_json(args_json) {
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
    let body_str = match resolve_pr_create_body(&v, working_dir) {
        Ok(b) => b,
        Err(e) => return e,
    };
    if let Err(e) = validate_pr_body(&body_str) {
        return e;
    }
    if let Err(e) = gh_pr_create_validate_repo_base_head(&v) {
        return e;
    }

    // 与历史文案对齐：路径非 UTF-8 时固定为「临时文件路径非 UTF-8」（不用带 label 的通用句）。
    let (dir, body_path_str) = match write_workspace_temp_markdown(
        working_dir,
        "crabmate_pr_body.md",
        body_str.as_bytes(),
        "PR 正文",
    ) {
        Ok(x) => x,
        Err(e) if e.contains("临时文件路径非 UTF-8") => {
            return "错误：临时文件路径非 UTF-8".to_string();
        }
        Err(e) => return e,
    };
    let argv = match gh_pr_create_build_argv(&v, title, body_path_str) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let out = run_gh_vec(argv, max_output_len, allowed_commands, working_dir);
    drop(dir);
    out
}
