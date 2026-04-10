use super::*;

use std::path::Path;

use crate::tool_result::ToolFailureCategory;

const TEST_COMMAND_MAX_OUTPUT_LEN: usize = 8192;
const TEST_WEATHER_TIMEOUT_SECS: u64 = 15;
fn test_ctx<'a>(allowed_commands: &'a [String]) -> ToolContext<'a> {
    ToolContext {
        codebase_semantic: None,
        command_max_output_len: TEST_COMMAND_MAX_OUTPUT_LEN,
        weather_timeout_secs: TEST_WEATHER_TIMEOUT_SECS,
        allowed_commands,
        working_dir: test_work_dir(),
        web_search_timeout_secs: 15,
        web_search_provider: crate::config::WebSearchProvider::Brave,
        web_search_api_key: "",
        web_search_max_results: 5,
        http_fetch_allowed_prefixes: &[] as &[String],
        http_fetch_timeout_secs: 30,
        http_fetch_max_response_bytes: 8192,
        command_timeout_secs: 30,
        read_file_turn_cache: None,
        workspace_changelist: None,
        test_result_cache_enabled: false,
        test_result_cache_max_entries: 8,
    }
}
fn test_allowed_commands() -> Vec<String> {
    vec![
        "ls".into(),
        "pwd".into(),
        "whoami".into(),
        "date".into(),
        "echo".into(),
        "id".into(),
        "uname".into(),
        "env".into(),
        "file".into(),
        "find".into(),
        "df".into(),
        "du".into(),
        "head".into(),
        "tail".into(),
        "wc".into(),
        "cat".into(),
        "cmake".into(),
        "ctest".into(),
        "mkdir".into(),
        "ninja".into(),
        "gcc".into(),
        "g++".into(),
        "clang".into(),
        "clang++".into(),
        "c++filt".into(),
        "autoreconf".into(),
        "autoconf".into(),
        "automake".into(),
        "aclocal".into(),
        "make".into(),
    ]
}
fn test_work_dir() -> &'static Path {
    Path::new(".")
}

#[test]
fn test_run_tool_unknown() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool("unknown_tool", "{}", &ctx);
    assert_eq!(out, "未知工具：unknown_tool");
}

#[test]
fn test_run_tool_try_unknown_is_err() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let e = run_tool_try("unknown_tool", "{}", &ctx).expect_err("unknown tool");
    assert_eq!(e.code, "unknown_tool");
    assert!(!e.retryable);
}

#[test]
fn test_run_tool_try_calc_ok() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool_try("calc", r#"{"expression":"1+1"}"#, &ctx).expect("calc");
    assert!(out.contains('2'), "got {out:?}");
}

#[test]
fn test_run_tool_calc_missing_expression() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool("calc", "{}", &ctx);
    assert_eq!(out, "错误：缺少 expression 参数");
}

#[test]
fn test_run_tool_calc_expression() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool("calc", r#"{"expression":"1+1"}"#, &ctx);
    assert!(out.contains("2"), "calc 1+1 应得到 2，得到: {}", out);
}

#[test]
fn test_run_tool_get_current_time() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool("get_current_time", "{}", &ctx);
    assert!(
        out.contains("当前时间"),
        "时间工具应包含「当前时间」，得到: {}",
        out
    );
}

#[test]
fn test_run_tool_convert_units() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool(
        "convert_units",
        r#"{"category":"length","value":1,"from":"km","to":"mile"}"#,
        &ctx,
    );
    assert!(out.contains("换算结果"), "应成功换算，得到: {out}");
}

#[test]
fn test_run_tool_typos_check_invokes_cli() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool("typos_check", r#"{"paths":["README.md"]}"#, &ctx);
    assert!(
        out.starts_with("typos (exit=") || out.contains("无法启动"),
        "应调用 typos 或报告未安装，得到: {out}"
    );
}

#[test]
fn test_run_tool_run_command_pwd() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool("run_command", r#"{"command":"pwd"}"#, &ctx);
    assert!(out.contains("退出码：0"), "pwd 应成功，得到: {}", out);
}

#[test]
fn test_run_tool_run_command_find_maxdepth() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool(
        "run_command",
        r#"{"command":"find","args":[".","-maxdepth","1","-type","f"]}"#,
        &ctx,
    );
    assert!(
        out.contains("退出码：0") || out.contains("无法启动") || out.contains("未找到命令"),
        "find 应成功或未安装，得到: {out}"
    );
}

#[test]
fn test_run_tool_run_command_disallowed() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool("run_command", r#"{"command":"rm"}"#, &ctx);
    assert!(out.contains("不允许的命令"), "应拒绝 rm，得到: {}", out);
}

#[test]
fn test_run_tool_try_run_command_disallowed_tool_error() {
    use crate::tool_result::ToolFailureCategory;
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let e = run_tool_try("run_command", r#"{"command":"rm"}"#, &ctx).expect_err("disallowed");
    assert_eq!(e.code, "command_not_allowed");
    assert_eq!(e.category, ToolFailureCategory::PolicyDenied);
    assert!(!e.retryable);
}

#[test]
fn test_run_tool_try_cargo_check_workspace_no_cargo_toml() {
    use crate::tool_result::ToolFailureCategory;
    let allowed = test_allowed_commands();
    let dir = std::env::temp_dir().join(format!("crabmate_cargo_empty_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir temp workspace");
    let ctx = ToolContext {
        working_dir: &dir,
        ..test_ctx(&allowed)
    };
    let e = run_tool_try("cargo_check", "{}", &ctx).expect_err("no Cargo.toml");
    assert_eq!(e.code, "workspace_no_cargo_toml");
    assert_eq!(e.category, ToolFailureCategory::Workspace);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_run_tool_get_weather_missing_param() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool("get_weather", "{}", &ctx);
    assert!(
        out.contains("city") || out.contains("location"),
        "缺少参数应提示，得到: {}",
        out
    );
}

#[test]
fn test_run_tool_web_search_no_api_key() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool("web_search", r#"{"query":"Rust programming"}"#, &ctx);
    assert!(
        out.contains("未配置") && out.contains("web_search"),
        "无 Key 时应提示配置，得到: {}",
        out
    );
}

#[test]
fn test_run_tool_package_query_smoke() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool("package_query", r#"{"package":"bash"}"#, &ctx);
    if out.trim_start().starts_with('{') {
        let v: serde_json::Value = serde_json::from_str(&out).expect("package_query 输出应为 JSON");
        assert_eq!(v.get("package").and_then(|x| x.as_str()), Some("bash"));
        assert!(v.get("installed").is_some());
        assert!(v.get("manager").is_some());
    } else {
        assert!(
            out.contains("未检测到可用的包管理查询命令"),
            "非 JSON 输出应是缺少包管理器的说明，得到: {}",
            out
        );
    }
}

#[test]
fn test_build_tools_names() {
    let tools = build_tools();
    let names: Vec<_> = tools.iter().map(|t| t.function.name.as_str()).collect();
    assert!(names.contains(&"get_current_time"));
    assert!(names.contains(&"calc"));
    assert!(names.contains(&"convert_units"));
    assert!(names.contains(&"get_weather"));
    assert!(names.contains(&"web_search"));
    assert!(names.contains(&"http_fetch"));
    assert!(names.contains(&"http_request"));
    assert!(names.contains(&"run_command"));
    assert!(names.contains(&"gh_pr_list"));
    assert!(names.contains(&"gh_pr_view"));
    assert!(names.contains(&"gh_issue_list"));
    assert!(names.contains(&"gh_issue_view"));
    assert!(names.contains(&"gh_run_list"));
    assert!(names.contains(&"gh_api"));
    assert!(names.contains(&"gh_pr_diff"));
    assert!(names.contains(&"gh_run_view"));
    assert!(names.contains(&"gh_release_list"));
    assert!(names.contains(&"gh_release_view"));
    assert!(names.contains(&"gh_search"));
    assert!(names.contains(&"cargo_check"));
    assert!(names.contains(&"cargo_test"));
    assert!(names.contains(&"cargo_clippy"));
    assert!(names.contains(&"cargo_metadata"));
    assert!(names.contains(&"cargo_tree"));
    assert!(names.contains(&"cargo_clean"));
    assert!(names.contains(&"cargo_doc"));
    assert!(names.contains(&"cargo_run"));
    assert!(names.contains(&"cargo_nextest"));
    assert!(names.contains(&"cargo_fmt_check"));
    assert!(names.contains(&"cargo_outdated"));
    assert!(names.contains(&"cargo_machete"));
    assert!(names.contains(&"cargo_udeps"));
    assert!(names.contains(&"cargo_publish_dry_run"));
    assert!(names.contains(&"rust_compiler_json"));
    assert!(names.contains(&"rust_analyzer_goto_definition"));
    assert!(names.contains(&"rust_analyzer_find_references"));
    assert!(names.contains(&"rust_analyzer_hover"));
    assert!(names.contains(&"rust_analyzer_document_symbol"));
    assert!(names.contains(&"cargo_fix"));
    assert!(names.contains(&"rust_test_one"));
    assert!(names.contains(&"ruff_check"));
    assert!(names.contains(&"pytest_run"));
    assert!(names.contains(&"mypy_check"));
    assert!(names.contains(&"python_install_editable"));
    assert!(names.contains(&"uv_sync"));
    assert!(names.contains(&"uv_run"));
    assert!(names.contains(&"python_snippet_run"));
    assert!(names.contains(&"pre_commit_run"));
    assert!(names.contains(&"typos_check"));
    assert!(names.contains(&"codespell_check"));
    assert!(names.contains(&"ast_grep_run"));
    assert!(names.contains(&"ast_grep_rewrite"));
    assert!(names.contains(&"frontend_lint"));
    assert!(names.contains(&"frontend_build"));
    assert!(names.contains(&"frontend_test"));
    assert!(names.contains(&"cargo_audit"));
    assert!(names.contains(&"cargo_deny"));
    assert!(names.contains(&"ci_pipeline_local"));
    assert!(names.contains(&"release_ready_check"));
    assert!(names.contains(&"workflow_execute"));
    assert!(names.contains(&"rust_backtrace_analyze"));
    assert!(names.contains(&"diagnostic_summary"));
    assert!(names.contains(&"error_output_playbook"));
    assert!(names.contains(&"playbook_run_commands"));
    assert!(names.contains(&"changelog_draft"));
    assert!(names.contains(&"license_notice"));
    assert!(names.contains(&"repo_overview_sweep"));
    assert!(names.contains(&"docs_health_sweep"));
    assert!(names.contains(&"git_status"));
    assert!(names.contains(&"git_clean_check"));
    assert!(names.contains(&"git_diff"));
    assert!(names.contains(&"git_diff_stat"));
    assert!(names.contains(&"git_diff_names"));
    assert!(names.contains(&"git_log"));
    assert!(names.contains(&"git_show"));
    assert!(names.contains(&"git_diff_base"));
    assert!(names.contains(&"git_blame"));
    assert!(names.contains(&"git_file_history"));
    assert!(names.contains(&"git_branch_list"));
    assert!(names.contains(&"git_remote_status"));
    assert!(names.contains(&"git_stage_files"));
    assert!(names.contains(&"git_commit"));
    assert!(names.contains(&"git_fetch"));
    assert!(names.contains(&"git_remote_list"));
    assert!(names.contains(&"git_remote_set_url"));
    assert!(names.contains(&"git_apply"));
    assert!(names.contains(&"git_clone"));
    assert!(names.contains(&"create_file"));
    assert!(names.contains(&"modify_file"));
    assert!(names.contains(&"copy_file"));
    assert!(names.contains(&"move_file"));
    assert!(names.contains(&"read_file"));
    assert!(names.contains(&"read_dir"));
    assert!(names.contains(&"glob_files"));
    assert!(names.contains(&"codebase_semantic_search"));
    assert!(names.contains(&"list_tree"));
    assert!(names.contains(&"file_exists"));
    assert!(names.contains(&"read_binary_meta"));
    assert!(names.contains(&"hash_file"));
    assert!(names.contains(&"extract_in_file"));
    assert!(names.contains(&"markdown_check_links"));
    assert!(names.contains(&"structured_validate"));
    assert!(names.contains(&"structured_query"));
    assert!(names.contains(&"structured_diff"));
    assert!(names.contains(&"structured_patch"));
    assert!(names.contains(&"text_transform"));
    assert!(names.contains(&"text_diff"));
    assert!(names.contains(&"table_text"));
    assert!(names.contains(&"find_symbol"));
    assert!(names.contains(&"find_references"));
    assert!(names.contains(&"rust_file_outline"));
    assert!(names.contains(&"format_file"));
    assert!(names.contains(&"format_check_file"));
    assert!(names.contains(&"run_lints"));
    assert!(names.contains(&"quality_workspace"));
    assert!(names.contains(&"maven_compile"));
    assert!(names.contains(&"maven_test"));
    assert!(names.contains(&"gradle_compile"));
    assert!(names.contains(&"gradle_test"));
    assert!(names.contains(&"docker_build"));
    assert!(names.contains(&"docker_compose_ps"));
    assert!(names.contains(&"podman_images"));
    assert!(names.contains(&"apply_patch"));
    assert!(names.contains(&"run_executable"));
    assert!(names.contains(&"package_query"));
}

#[test]
fn test_build_tools_filtered_basic_vs_development() {
    let basic = build_tools_filtered(Some(&[ToolCategory::Basic]));
    let dev = build_tools_filtered(Some(&[ToolCategory::Development]));
    let full = build_tools();
    assert_eq!(basic.len() + dev.len(), full.len());
    let bn: Vec<_> = basic.iter().map(|t| t.function.name.as_str()).collect();
    assert!(bn.contains(&"get_current_time"));
    assert!(bn.contains(&"convert_units"));
    assert!(bn.contains(&"text_transform"));
    assert!(bn.contains(&"add_reminder"));
    assert!(!bn.contains(&"cargo_check"));
    let dn: Vec<_> = dev.iter().map(|t| t.function.name.as_str()).collect();
    assert!(dn.contains(&"cargo_check"));
    assert!(dn.contains(&"text_diff"));
    assert!(!dn.contains(&"get_current_time"));
}

#[test]
fn test_build_tools_dev_tags_rust_excludes_pure_vcs() {
    let rust_only = [dev_tag::RUST];
    let tools = build_tools_with_options(ToolsBuildOptions {
        categories: Some(&[ToolCategory::Development]),
        dev_tags: Some(rust_only.as_slice()),
    });
    let names: Vec<_> = tools.iter().map(|t| t.function.name.as_str()).collect();
    assert!(names.contains(&"cargo_check"));
    assert!(!names.contains(&"git_status"));
}

#[test]
fn test_summarize_tool_call_static() {
    let s = summarize_tool_call("cargo_check", "{}");
    assert_eq!(s, Some("cargo check".to_string()));
}

#[test]
fn test_summarize_tool_call_dynamic() {
    let s = summarize_tool_call("create_file", r#"{"path":"src/foo.rs","content":"hello"}"#);
    assert_eq!(s, Some("create file: src/foo.rs".to_string()));
}

#[test]
fn test_summarize_tool_call_dynamic_run_command() {
    let s = summarize_tool_call("run_command", r#"{"command":"ls","args":["-la"]}"#);
    assert_eq!(s, Some("ls -la".to_string()));
}

#[test]
fn test_summarize_search_in_files_truncates_long_pattern() {
    let pat = "x".repeat(80);
    let s = summarize_tool_call("search_in_files", &format!(r#"{{"pattern":"{pat}"}}"#))
        .expect("summary");
    assert!(s.starts_with("search in files: "));
    assert!(s.ends_with('…'), "expected ellipsis for long pattern: {s}");
    assert!(
        s.chars().count() <= 64,
        "summary should stay short, got {} chars: {s}",
        s.chars().count()
    );
}

#[test]
fn test_summarize_search_in_files_with_path_short() {
    let s = summarize_tool_call("search_in_files", r#"{"pattern":"fn main","path":"src"}"#)
        .expect("summary");
    assert_eq!(s, "search in files: fn main @ src");
}

#[test]
fn test_read_file_try_workspace_error_has_stable_code() {
    let dir = tempfile::tempdir().expect("tempdir");
    let parent = dir.path().parent().expect("parent");
    let outside_name = format!("crabmate_read_escape_{}.txt", std::process::id());
    let outside_path = parent.join(&outside_name);
    std::fs::write(&outside_path, "x\n").expect("write outside");
    let args = serde_json::json!({ "path": format!("../{}", outside_name) }).to_string();
    let allowed = test_allowed_commands();
    let mut ctx = test_ctx(&allowed);
    ctx.working_dir = dir.path();
    let err = run_tool_try("read_file", &args, &ctx).expect_err("outside workspace");
    assert!(
        err.code.starts_with("read_file_workspace_"),
        "expected read_file_workspace_* code, got {}",
        err.code
    );
    assert_eq!(err.category, ToolFailureCategory::Workspace);
    let _ = std::fs::remove_file(&outside_path);
}

#[test]
fn test_search_in_files_try_invalid_regex_error_code() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let err =
        run_tool_try("search_in_files", r#"{"pattern":"("}"#, &ctx).expect_err("invalid regex");
    assert_eq!(err.code, "search_in_files_invalid_regex");
    assert_eq!(err.category, ToolFailureCategory::InvalidInput);
}

#[test]
fn test_summarize_tool_call_none() {
    let s = summarize_tool_call("get_current_time", "{}");
    assert_eq!(s, None);
}

#[test]
fn test_summarize_tool_call_unknown_tool() {
    let s = summarize_tool_call("nonexistent_tool_xyz", "{}");
    assert_eq!(s, None);
}

#[test]
fn test_error_output_playbook_respects_command_whitelist() {
    let allowed = vec!["cargo".to_string()];
    let ctx = test_ctx(&allowed);
    let out = run_tool(
        "error_output_playbook",
        r#"{"error_text":"error[E0599]: no method named `x`","ecosystem":"rust"}"#,
        &ctx,
    );
    assert!(
        out.contains("cargo check") || out.contains("cargo build"),
        "应建议 cargo 子命令，得到: {}",
        out
    );
    assert!(
        !out.contains("python3"),
        "白名单无 python3 时不应出现 python3 建议: {}",
        out
    );
}

#[test]
fn test_build_tools_dev_tags_basic_plus_rust() {
    let rust_only = [dev_tag::RUST];
    let tools = build_tools_with_options(ToolsBuildOptions {
        categories: Some(&[ToolCategory::Basic, ToolCategory::Development]),
        dev_tags: Some(rust_only.as_slice()),
    });
    let names: Vec<_> = tools.iter().map(|t| t.function.name.as_str()).collect();
    assert!(names.contains(&"calc"));
    assert!(names.contains(&"convert_units"));
    assert!(names.contains(&"cargo_check"));
    assert!(!names.contains(&"git_status"));
}

#[test]
fn repl_workspace_switch_rejects_slash_prefixed_as_tool_relative() {
    let cfg = crate::config::load_config(None).expect("embedded default config");
    let wd = cfg
        .workspace_allowed_roots
        .first()
        .cloned()
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
    let err = resolve_repl_workspace_switch_path(&cfg, &wd, "/tmp").expect_err("must reject");
    let msg = err.to_string();
    assert!(
        msg.contains("允许范围")
            || msg.contains("敏感")
            || msg.contains("绝对路径")
            || msg.contains("相对路径"),
        "{msg}"
    );
}

#[test]
fn repl_workspace_switch_relative_subdir_resolves_like_read_file() {
    let cfg = crate::config::load_config(None).expect("embedded default config");
    let wd = cfg
        .workspace_allowed_roots
        .first()
        .cloned()
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
    let sub = "src";
    if !wd.join(sub).is_dir() {
        return;
    }
    let got =
        resolve_repl_workspace_switch_path(&cfg, &wd, sub).expect("expected src under workspace");
    assert!(got.is_dir());
}
