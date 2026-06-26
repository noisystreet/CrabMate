use std::path::Path;

use super::common::{
    attach_json_if_exit_zero, clamp_limit, extract_stdout_from_formatted, gh_allowed,
    join_json_fields, run_gh_vec, validate_extra_args, validate_pr_body, validate_pr_ref_token,
    validate_pr_title, validate_repo,
};
use super::pr_body::build_pr_body_draft;
use super::run_ci::append_checks_summary;

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
    let mut argv = vec!["pr".into(), "checks".into()];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if let Err(e) = validate_repo(r) {
            return e;
        }
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    if let Some(n) = v.get("number").and_then(|x| x.as_u64()) {
        if n == 0 || n > 999_999 {
            return "错误：number 须为 1～999999 的正整数或省略".to_string();
        }
        argv.push(n.to_string());
    }
    if v.get("structured").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--json".into());
        argv.push("name,bucket,state,link,workflow,description,startedAt,completedAt,event".into());
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
    let out = run_gh_vec(argv, max_output_len, allowed_commands, working_dir);
    if v.get("structured").and_then(|x| x.as_bool()) == Some(true) {
        let stdout = extract_stdout_from_formatted(&out).to_string();
        return append_checks_summary(
            attach_json_if_exit_zero(out, stdout.as_str()),
            stdout.as_str(),
        );
    }
    out
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
    let auto_body = v.get("auto_body").and_then(|x| x.as_bool()).unwrap_or(true);
    let body_str = match v.get("body").and_then(|x| x.as_str()) {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ if auto_body => {
            let base = v.get("base").and_then(|x| x.as_str());
            match build_pr_body_draft(working_dir, base, 30, true, true) {
                Ok(d) => d,
                Err(e) => return e,
            }
        }
        _ => String::new(),
    };
    if let Err(e) = validate_pr_body(&body_str) {
        return e;
    }
    if let Err(e) = gh_pr_create_validate_repo_base_head(&v) {
        return e;
    }

    let dir = match tempfile::tempdir_in(working_dir) {
        Ok(d) => d,
        Err(e) => return format!("错误：无法在工作区内创建临时目录：{e}"),
    };
    let body_path = dir.path().join("crabmate_pr_body.md");
    if let Err(e) = std::fs::write(&body_path, body_str.as_bytes()) {
        return format!("错误：写入 PR 正文临时文件失败：{e}");
    }
    let body_path_str = match body_path.to_str() {
        Some(p) => p.to_string(),
        None => return "错误：临时文件路径非 UTF-8".to_string(),
    };

    let argv = match gh_pr_create_build_argv(&v, title, body_path_str) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let out = run_gh_vec(argv, max_output_len, allowed_commands, working_dir);
    drop(dir);
    out
}

fn gh_pr_create_validate_repo_base_head(v: &serde_json::Value) -> Result<(), String> {
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
    v: &serde_json::Value,
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
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    if let Some(b) = v.get("base").and_then(|x| x.as_str()) {
        argv.push("--base".into());
        argv.push(b.trim().to_string());
    }
    if let Some(h) = v.get("head").and_then(|x| x.as_str()) {
        argv.push("--head".into());
        argv.push(h.trim().to_string());
    }
    if v.get("draft").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--draft".into());
    }
    if v.get("web").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--web".into());
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
