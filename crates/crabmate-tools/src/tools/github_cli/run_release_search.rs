use std::path::Path;

use serde_json::Value as JsonValue;

use super::common::{
    clamp_limit, clamp_search_limit, gh_allowed, push_bool_flag, push_extra_args_from_json,
    push_json_fields_from_json, push_repo_arg, run_gh_vec, validate_job_name, validate_release_tag,
    validate_run_id, validate_search_query,
};

fn build_gh_run_view_argv(v: &JsonValue) -> Result<Vec<String>, String> {
    let run_id = match v.get("run_id").and_then(|x| x.as_str()) {
        Some(s) => s.trim(),
        None => return Err("错误：缺少 run_id".to_string()),
    };
    validate_run_id(run_id)?;
    let mut argv = vec!["run".into(), "view".into(), run_id.to_string()];
    push_repo_arg(v, &mut argv)?;
    if v.get("log").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--log".into());
        if let Some(j) = v.get("job").and_then(|x| x.as_str()) {
            validate_job_name(j)?;
            argv.push("--job".into());
            argv.push(j.trim().to_string());
        }
    }
    push_json_fields_from_json(v, &mut argv)?;
    push_bool_flag(v, "web", "--web", &mut argv);
    push_extra_args_from_json(v, &mut argv)?;
    Ok(argv)
}

/// `gh run view`（日志/摘要；输出受 `command_max_output_len` 截断）
pub fn gh_run_view(
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
    let argv = match build_gh_run_view_argv(&v) {
        Ok(a) => a,
        Err(e) => return e,
    };
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

fn build_gh_release_list_argv(v: &JsonValue) -> Result<Vec<String>, String> {
    let mut argv = vec!["release".into(), "list".into()];
    push_repo_arg(v, &mut argv)?;
    let lim = clamp_limit(v.get("limit").and_then(|x| x.as_u64()).map(|u| u as u32));
    argv.push("--limit".into());
    argv.push(lim.to_string());
    push_json_fields_from_json(v, &mut argv)?;
    push_bool_flag(v, "web", "--web", &mut argv);
    push_extra_args_from_json(v, &mut argv)?;
    Ok(argv)
}

/// `gh release list`
pub fn gh_release_list(
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
    let argv = match build_gh_release_list_argv(&v) {
        Ok(a) => a,
        Err(e) => return e,
    };
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

fn build_gh_release_view_argv(v: &JsonValue) -> Result<Vec<String>, String> {
    let tag = match v.get("tag").and_then(|x| x.as_str()) {
        Some(s) => s.trim(),
        None => return Err("错误：缺少 tag".to_string()),
    };
    validate_release_tag(tag)?;
    let mut argv = vec!["release".into(), "view".into(), tag.to_string()];
    push_repo_arg(v, &mut argv)?;
    push_json_fields_from_json(v, &mut argv)?;
    push_bool_flag(v, "web", "--web", &mut argv);
    push_extra_args_from_json(v, &mut argv)?;
    Ok(argv)
}

/// `gh release view`
pub fn gh_release_view(
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
    let argv = match build_gh_release_view_argv(&v) {
        Ok(a) => a,
        Err(e) => return e,
    };
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

fn build_gh_search_argv(v: &JsonValue) -> Result<Vec<String>, String> {
    let scope = match v.get("scope").and_then(|x| x.as_str()) {
        Some(s) => s.trim(),
        None => return Err("错误：缺少 scope".to_string()),
    };
    if !matches!(scope, "issues" | "prs" | "repos") {
        return Err("错误：scope 须为 issues、prs 或 repos".to_string());
    }
    let q = match v.get("query").and_then(|x| x.as_str()) {
        Some(s) => s,
        None => return Err("错误：缺少 query".to_string()),
    };
    validate_search_query(q)?;
    let mut argv = vec!["search".into(), scope.into(), q.trim().to_string()];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if scope != "repos" {
            super::common::validate_repo(r)?;
            argv.push("--repo".into());
            argv.push(r.trim().to_string());
        } else {
            return Err("错误：scope=repos 时不要使用 repo 参数".to_string());
        }
    }
    let lim = clamp_search_limit(v.get("limit").and_then(|x| x.as_u64()).map(|u| u as u32));
    argv.push("--limit".into());
    argv.push(lim.to_string());
    push_json_fields_from_json(v, &mut argv)?;
    push_extra_args_from_json(v, &mut argv)?;
    Ok(argv)
}

/// `gh search`（仅允许 issues / prs / repos）
pub fn gh_search(
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
    let argv = match build_gh_search_argv(&v) {
        Ok(a) => a,
        Err(e) => return e,
    };
    run_gh_vec(argv, max_output_len, allowed_commands, working_dir)
}

#[cfg(test)]
mod tests {
    use super::super::common::{
        validate_pr_ref_token, validate_pr_title, validate_repo, validate_run_id,
        validate_search_query,
    };
    use super::super::{attach_json_if_exit_zero, gh_pr_checks, gh_pr_list, validate_api_path};
    use super::build_gh_run_view_argv;

    fn allowed() -> Vec<String> {
        vec!["gh".into()]
    }

    #[test]
    fn validate_repo_rejects_absolute() {
        assert!(validate_repo("/a/b").is_err());
        assert!(validate_repo("a/../b").is_err());
        assert!(validate_repo("o/r").is_ok());
    }

    #[test]
    fn validate_api_path_cases() {
        assert!(validate_api_path("repos/foo/bar/issues").is_ok());
        assert!(validate_api_path("/repos/x").is_err());
        assert!(validate_api_path("repos/../x").is_err());
    }

    #[test]
    fn attach_json_if_exit_zero_appends_on_json_stdout() {
        let raw = "退出码：0\n标准输出：\n[1,2]\n".to_string();
        let out = attach_json_if_exit_zero(raw, "[1,2]");
        assert!(out.contains("解析后的 JSON"), "{}", out);
    }

    #[test]
    fn attach_json_skips_on_nonzero_exit() {
        let raw = "退出码：1\n标准输出：\n{}\n".to_string();
        let out = attach_json_if_exit_zero(raw, "{}");
        assert!(!out.contains("解析后的 JSON"), "{}", out);
    }

    #[test]
    fn validate_search_query_rejects_shell_chars() {
        assert!(validate_search_query("foo;rm").is_err());
        assert!(validate_search_query("repo:foo/bar").is_ok());
    }

    #[test]
    fn validate_pr_title_rejects_newline() {
        assert!(validate_pr_title("a\nb").is_err());
        assert!(validate_pr_title("ok title").is_ok());
    }

    #[test]
    fn validate_pr_ref_token_rejects_dotdot() {
        assert!(validate_pr_ref_token("main..other").is_err());
        assert!(validate_pr_ref_token("feature/foo").is_ok());
        assert!(validate_pr_ref_token("fork:branch").is_ok());
    }

    #[test]
    fn gh_pr_checks_requires_gh_in_allowlist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = gh_pr_checks("{}", 4096, &[], dir.path());
        assert!(out.contains("未包含 gh"), "{}", out);
    }

    #[test]
    fn validate_run_id_numeric() {
        assert!(validate_run_id("12345").is_ok());
        assert!(validate_run_id("12a").is_err());
    }

    #[test]
    fn gh_pr_list_requires_gh_in_allowlist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = gh_pr_list("{}", 4096, &[], dir.path());
        assert!(out.contains("未包含 gh"), "{}", out);
    }

    #[test]
    fn gh_pr_list_invokes_gh_or_errors() {
        crate::tools::command::reset_run_command_rate_limit_for_tests();
        let dir = tempfile::tempdir().expect("tempdir");
        let out = gh_pr_list(
            r#"{"limit":1,"fields":["number","title"]}"#,
            8192,
            &allowed(),
            dir.path(),
        );
        assert!(
            out.contains("退出码：")
                || out.contains("无法执行")
                || out.contains("不存在")
                || out.contains("过于频繁"),
            "unexpected: {out}"
        );
    }

    #[test]
    fn build_gh_run_view_argv_log_and_job() {
        let v = serde_json::json!({
            "run_id": "42",
            "repo": "o/r",
            "log": true,
            "job": "build",
            "web": true,
            "fields": ["databaseId", "url"],
        });
        let argv = build_gh_run_view_argv(&v).expect("argv");
        assert_eq!(
            argv,
            vec![
                "run",
                "view",
                "42",
                "-R",
                "o/r",
                "--log",
                "--job",
                "build",
                "--json",
                "databaseId,url",
                "--web",
            ]
        );
    }

    #[test]
    fn build_gh_run_view_argv_rejects_bad_run_id() {
        let v = serde_json::json!({ "run_id": "../x" });
        assert!(build_gh_run_view_argv(&v).is_err());
    }

    #[test]
    fn build_gh_run_view_argv_blank_run_id_matches_validate_message() {
        let v = serde_json::json!({ "run_id": "   " });
        let err = build_gh_run_view_argv(&v).expect_err("blank");
        assert!(
            err.contains("run_id 不能为空"),
            "expected validate_run_id message, got {err}"
        );
    }
}
