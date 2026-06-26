use std::path::Path;

use super::common::{
    clamp_limit, clamp_search_limit, gh_allowed, join_json_fields, run_gh_vec, validate_extra_args,
    validate_job_name, validate_release_tag, validate_repo, validate_run_id, validate_search_query,
};

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
    let run_id = match v.get("run_id").and_then(|x| x.as_str()) {
        Some(s) => s.trim(),
        None => return "错误：缺少 run_id".to_string(),
    };
    if let Err(e) = validate_run_id(run_id) {
        return e;
    }
    let mut argv = vec!["run".into(), "view".into(), run_id.to_string()];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if let Err(e) = validate_repo(r) {
            return e;
        }
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
    if v.get("log").and_then(|x| x.as_bool()) == Some(true) {
        argv.push("--log".into());
        if let Some(j) = v.get("job").and_then(|x| x.as_str()) {
            if let Err(e) = validate_job_name(j) {
                return e;
            }
            argv.push("--job".into());
            argv.push(j.trim().to_string());
        }
    }
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
    let mut argv = vec!["release".into(), "list".into()];
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
    let tag = match v.get("tag").and_then(|x| x.as_str()) {
        Some(s) => s.trim(),
        None => return "错误：缺少 tag".to_string(),
    };
    if let Err(e) = validate_release_tag(tag) {
        return e;
    }
    let mut argv = vec!["release".into(), "view".into(), tag.to_string()];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if let Err(e) = validate_repo(r) {
            return e;
        }
        argv.push("-R".into());
        argv.push(r.trim().to_string());
    }
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
    let scope = match v.get("scope").and_then(|x| x.as_str()) {
        Some(s) => s.trim(),
        None => return "错误：缺少 scope".to_string(),
    };
    if !matches!(scope, "issues" | "prs" | "repos") {
        return "错误：scope 须为 issues、prs 或 repos".to_string();
    }
    let q = match v.get("query").and_then(|x| x.as_str()) {
        Some(s) => s,
        None => return "错误：缺少 query".to_string(),
    };
    if let Err(e) = validate_search_query(q) {
        return e;
    }
    let mut argv = vec!["search".into(), scope.into(), q.trim().to_string()];
    if let Some(r) = v.get("repo").and_then(|x| x.as_str()) {
        if scope != "repos" {
            if let Err(e) = validate_repo(r) {
                return e;
            }
            argv.push("--repo".into());
            argv.push(r.trim().to_string());
        } else {
            return "错误：scope=repos 时不要使用 repo 参数".to_string();
        }
    }
    let lim = clamp_search_limit(v.get("limit").and_then(|x| x.as_u64()).map(|u| u as u32));
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

#[cfg(test)]
mod tests {
    use super::super::common::{
        validate_pr_ref_token, validate_pr_title, validate_repo, validate_run_id,
        validate_search_query,
    };
    use super::super::{attach_json_if_exit_zero, gh_pr_checks, gh_pr_list, validate_api_path};

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
        let dir = tempfile::tempdir().expect("tempdir");
        let out = gh_pr_list(
            r#"{"limit":1,"fields":["number","title"]}"#,
            8192,
            &allowed(),
            dir.path(),
        );
        assert!(
            out.contains("退出码：") || out.contains("无法执行") || out.contains("不存在"),
            "unexpected: {out}"
        );
    }
}
