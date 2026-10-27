use super::*;

use std::path::Path;

use crate::cm_tools::tool_result::{ToolFailureCategory, parse_legacy_output};

const TEST_COMMAND_MAX_OUTPUT_LEN: usize = 8192;
const TEST_WEATHER_TIMEOUT_SECS: u64 = 15;
fn test_ctx<'a>(allowed_commands: &'a [String]) -> ToolContext<'a> {
    ToolContext {
        cfg: None,
        codebase_semantic_host: None,
        command_max_output_len: TEST_COMMAND_MAX_OUTPUT_LEN,
        weather_timeout_secs: TEST_WEATHER_TIMEOUT_SECS,
        allowed_commands,
        working_dir: test_work_dir(),
        path_scope: crate::cm_types::PathScope::SessionRoot,
        web_search_timeout_secs: 15,
        web_search_provider: crate::cm_config::WebSearchProvider::Brave,
        web_search_api_key: "",
        web_search_max_results: 5,
        web_search_proxy: "",
        http_fetch_allowed_prefixes: &[] as &[String],
        http_fetch_timeout_secs: 30,
        http_fetch_max_response_bytes: 8192,
        command_timeout_secs: 30,
        read_file_turn_cache: None,
        workspace_changelist: None,
        test_result_cache_enabled: false,
        test_result_cache_max_entries: 8,
        long_term_memory_host: None,
        tool_jobs_host: None,
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
fn test_run_tool_try_schema_rejects_wrong_type() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let e = run_tool_try("json_format", r#"{"text":1}"#, &ctx).expect_err("schema");
    assert_eq!(e.code, "invalid_args");
    assert!(e.message.contains("JSON Schema"), "{}", e.message);
}


#[tokio::test]
async fn test_run_tool_run_command_pwd() {
    crate::cm_tools::tools::command::reset_run_command_rate_limit_for_tests();
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool_async("run_command", r#"{"command":"pwd"}"#, &ctx).await;
    assert!(out.contains("命令：pwd"), "应含命令行，得到: {}", out);
    assert!(out.contains("退出码：0"), "pwd 应成功，得到: {}", out);
}

#[tokio::test]
async fn test_run_tool_run_command_find_maxdepth() {
    crate::cm_tools::tools::command::reset_run_command_rate_limit_for_tests();
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool_async(
        "run_command",
        r#"{"command":"find","args":[".","-maxdepth","1","-type","f"]}"#,
        &ctx,
    )
    .await;
    assert!(
        out.contains("退出码：0") || out.contains("无法启动") || out.contains("未找到命令"),
        "find 应成功或未安装，得到: {out}"
    );
}

#[tokio::test]
async fn test_run_tool_run_command_disallowed() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool_async("run_command", r#"{"command":"rm"}"#, &ctx).await;
    assert!(out.contains("不允许的命令"), "应拒绝 rm，得到: {}", out);
}

#[tokio::test]
async fn test_run_tool_async_run_command_disallowed_tool_error() {
    use crate::cm_tools::tool_result::ToolFailureCategory;
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    // 直调私有异步分派以取回结构化 `ToolError`（`run_tool_result_async` 会降级为 `ToolResult`）。
    let e = run_command_dispatch_async("run_command", r#"{"command":"rm"}"#, &ctx)
        .await
        .expect_err("disallowed");
    assert_eq!(e.code, "approval_required");
    assert_eq!(e.category, ToolFailureCategory::PolicyDenied);
    assert!(!e.retryable);
}

#[test]
fn test_run_tool_try_cargo_check_workspace_no_cargo_toml() {
    use crate::cm_tools::tool_result::ToolFailureCategory;
    let allowed = test_allowed_commands();
    let dir = std::env::temp_dir().join(format!("crabmate_cargo_empty_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir temp workspace");
    let ctx = ToolContext {
        working_dir: &dir,
        path_scope: crate::cm_types::PathScope::SessionRoot,
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
fn test_run_tool_code_nav_call_graph_sketch_smoke() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool(
        "code_nav",
        r#"{"action":"call_graph_sketch","symbols":["run_tool"],"path":"src/cm_tools/tools","max_edges":80}"#,
        &ctx,
    );
    assert!(
        out.contains("变更影响面草图"),
        "应输出 Markdown 标题，得到: {}",
        out.chars().take(400).collect::<String>()
    );
    assert!(
        out.contains("reference") || out.contains("use") || out.contains("run_tool"),
        "应对 src/cm_tools/tools 下 run_tool 有命中或明细，得到: {}",
        out.chars().take(400).collect::<String>()
    );
}

#[test]
fn test_build_tools_names() {
    let tools = build_tools();
    let names: Vec<_> = tools.iter().map(|t| t.function.name.as_str()).collect();
    assert!(names.contains(&"get_weather"));
    assert!(names.contains(&"web_search"));
    assert!(names.contains(&"http_fetch"));
    assert!(names.contains(&"http_request"));
    assert!(names.contains(&"run_command"));
    assert!(names.contains(&"gh_pr"));
    assert!(names.contains(&"gh_run"));
    assert!(names.contains(&"gh_issue_create"));
    assert!(names.contains(&"gh_api"));
    assert!(names.contains(&"gh_release_create"));
    // 已移除的工具不应复活：gh_* 已合并为 gh_pr / gh_run；git_* 已合并为 git（action 选择器）；
    // format_files 已收敛到 run_command；rust_analyzer_* 八个已合并为单个 rust_analyzer。
    for gone in [
        "format_files",
        "gh_pr_checks",
        "gh_pr_create",
        "gh_pr_merge",
        "gh_pr_review",
        "gh_pr_comment",
        "gh_pr_edit",
        "gh_pr_body_draft",
        "gh_run_rerun",
        "gh_run_failure_summary",
        "rust_analyzer_goto_definition",
        "rust_analyzer_find_references",
        "rust_analyzer_hover",
        "rust_analyzer_document_symbol",
        "rust_analyzer_goto_implementation",
        "rust_analyzer_goto_type_definition",
        "rust_analyzer_document_highlight",
        "rust_analyzer_workspace_symbol",
        "git_status",
        "git_diff",
        "git_clean_check",
        "git_log",
        "git_show",
        "git_blame",
        "git_file_history",
        "git_branch_list",
        "git_remote_status",
        "git_remote_list",
        // code_nav 四合一：四个旧工具名不应复活
        "find_symbol",
        "find_references",
        "call_graph_sketch",
        "rust_file_outline",
    ] {
        assert!(!names.contains(&gone), "{gone} 已移除，不应重新出现");
    }
    assert!(names.contains(&"cargo_check"));
    assert!(names.contains(&"cargo_test"));
    assert!(names.contains(&"cargo_clippy"));
    assert!(names.contains(&"cargo_run"));
    assert!(names.contains(&"rust_compiler_json"));
    assert!(names.contains(&"rust_analyzer"));
    assert!(names.contains(&"rust_test_one"));
    assert!(names.contains(&"pytest_run"));
    assert!(names.contains(&"python_snippet_run"));
    assert!(names.contains(&"workflow_execute"));
    assert!(names.contains(&"rust_backtrace_analyze"));
    assert!(names.contains(&"diagnostic_summary"));
    assert!(names.contains(&"self_config_info"));
    assert!(names.contains(&"error_output_playbook"));
    assert!(names.contains(&"playbook_run_commands"));
    assert!(names.contains(&"changelog_draft"));
    assert!(names.contains(&"license_notice"));
    assert!(names.contains(&"repo_overview_sweep"));
    assert!(names.contains(&"crate_contract_map"));
    assert!(names.contains(&"docs_health_sweep"));
    assert!(names.contains(&"git"));
    assert!(names.contains(&"create_file"));
    assert!(names.contains(&"modify_file"));
    assert!(names.contains(&"copy_file"));
    assert!(names.contains(&"move_file"));
    assert!(names.contains(&"read_file"));
    assert!(names.contains(&"read_dir"));
    assert!(names.contains(&"glob_files"));
    #[cfg(feature = "fastembed")]
    assert!(names.contains(&"codebase_semantic_search"));
    #[cfg(not(feature = "fastembed"))]
    assert!(!names.contains(&"codebase_semantic_search"));
    assert!(names.contains(&"list_tree"));
    assert!(names.contains(&"read_binary_meta"));
    assert!(names.contains(&"hash_file"));
    assert!(names.contains(&"markdown_check_links"));
    assert!(names.contains(&"structured_validate"));
    assert!(names.contains(&"structured_query"));
    assert!(names.contains(&"structured_diff"));
    assert!(names.contains(&"structured_patch"));
    assert!(names.contains(&"text_transform"));
    assert!(names.contains(&"text_diff"));
    assert!(names.contains(&"table_text"));
    assert!(names.contains(&"code_nav"));
    assert!(names.contains(&"run_lints"));
    assert!(names.contains(&"apply_patch"));
}

#[test]
fn test_build_tools_filtered_basic_vs_development() {
    let basic = build_tools_filtered(Some(&[ToolCategory::Basic]));
    let dev = build_tools_filtered(Some(&[ToolCategory::Development]));
    let full = build_tools();
    assert_eq!(basic.len() + dev.len(), full.len());
    let bn: Vec<_> = basic.iter().map(|t| t.function.name.as_str()).collect();
    assert!(bn.contains(&"text_transform"));
    assert!(bn.contains(&"reminder"));
    assert!(!bn.contains(&"cargo_check"));
    let dn: Vec<_> = dev.iter().map(|t| t.function.name.as_str()).collect();
    assert!(dn.contains(&"cargo_check"));
    assert!(dn.contains(&"text_diff"));
    assert!(!dn.contains(&"text_transform"));
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

/// 模型把整条命令写成数组时摘要仍要出来：字段是 `String` 时反序列化失败，
/// 摘要会退化成占位串 `tool: run_command`，客户端只能再解析一遍参数补回来。
#[test]
fn test_summarize_tool_call_run_command_array_command() {
    let s = summarize_tool_call("run_command", r#"{"command":["git","status","-sb"]}"#);
    assert_eq!(s, Some("git status -sb".to_string()));
}

/// 数组只放程序名、参数另起 `args`：两者都要出现在摘要里。
#[test]
fn test_summarize_tool_call_run_command_array_plus_args() {
    let s = summarize_tool_call("run_command", r#"{"command":["git"],"args":["status","-sb"]}"#);
    assert_eq!(s, Some("git status -sb".to_string()));
}

/// `command` 为空串时仍返回 `None`，由上层决定是否用占位串。
#[test]
fn test_summarize_tool_call_run_command_empty_command() {
    let s = summarize_tool_call("run_command", r#"{"command":"","args":["ls"]}"#);
    assert_eq!(s, None);
}

/// 卡片行要用注册名 `search_replace`：曾硬编码 `search-replace{}: …`，客户端因此显示
/// 了一个工具列表里不存在的连字符名（TUI 还得再归一化一次才不重复打印名字）。
#[test]
fn test_summarize_tool_call_search_replace_preview_uses_registry_name() {
    let s = summarize_tool_call(
        "search_replace",
        r#"{"path":"a.rs","search":"foo","dry_run":true}"#,
    );
    assert_eq!(
        s.as_deref(),
        Some(r#"search_replace (preview): a.rs / "foo""#)
    );
}

/// 写入形态同样不带连字符，`(preview)` 后缀只在预览时出现。
#[test]
fn test_summarize_tool_call_search_replace_write_uses_registry_name() {
    let s = summarize_tool_call(
        "search_replace",
        r#"{"path":"a.rs","search":"foo","dry_run":false}"#,
    );
    assert_eq!(s.as_deref(), Some(r#"search_replace: a.rs / "foo""#));
}

#[test]
fn test_summarize_read_file_range_normalizes_inverted_order() {
    let s = summarize_tool_call(
        "read_file",
        r#"{"path":"frontend/src/api/mod.rs","start_line":419,"end_line":150}"#,
    )
    .expect("summary");
    assert!(
        s.contains("[150-419]"),
        "expected normalized range in summary: {s}"
    );
    assert!(
        !s.contains("[419-150]"),
        "should not echo inverted order: {s}"
    );
}

#[test]
fn test_summarize_search_in_files_truncates_long_pattern() {
    let pat = "x".repeat(80);
    let s = summarize_tool_call("search_in_files", &format!(r#"{{"pattern":"{pat}"}}"#))
        .expect("summary");
    assert!(s.starts_with("search in files: "));
    assert!(s.ends_with("…"), "expected ellipsis for long pattern: {s}");
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
fn test_read_file_count_total_rejects_oversized_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("big.txt");
    let f = std::fs::File::create(&path).expect("create");
    let oversized: u64 = 32 * 1024 * 1024 + 1;
    f.set_len(oversized).expect("set_len sparse");
    let args = serde_json::json!({
        "path": "big.txt",
        "count_total_lines": true
    })
    .to_string();
    let allowed = test_allowed_commands();
    let mut ctx = test_ctx(&allowed);
    ctx.working_dir = dir.path();
    let err = run_tool_try("read_file", &args, &ctx).expect_err("oversized count_total");
    assert_eq!(err.code, "read_file_count_total_too_large");
    assert_eq!(err.category, ToolFailureCategory::InvalidInput);
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
    let s = summarize_tool_call("text_transform", "{}");
    assert_eq!(s, None);
}

// ── `is_compile_command_success` 判定边界（S0.5-3：仅钉现状，修复留到 S5）──

fn legacy_run_output(command: &str, exit_code: i32) -> String {
    format!("命令：{command}\n退出码：{exit_code}\n")
}

#[test]
fn is_compile_command_success_requires_zero_exit_and_compile_command() {
    assert!(is_compile_command_success(
        r#"{"command":"gcc","args":["-c","a.c"]}"#,
        &legacy_run_output("gcc -c a.c", 0)
    ));
    assert!(!is_compile_command_success(
        r#"{"command":"gcc","args":["-c","a.c"]}"#,
        &legacy_run_output("gcc -c a.c", 1)
    ));
    assert!(!is_compile_command_success(
        r#"{"command":"ls","args":[]}"#,
        &legacy_run_output("ls", 0)
    ));
    assert!(!is_compile_command_success(
        "not json",
        &legacy_run_output("gcc -c a.c", 0)
    ));
}

#[test]
fn is_compile_command_success_recognizes_script_shaped_payload() {
    // S5：识别改为消费 `CommandSpec`，`bash -c "gcc …"`、`script` 字段与
    // command 为空、编译工具落在 args 首项的 payload 均可识别。
    assert!(is_compile_command_success(
        r#"{"command":"bash","args":["-c","gcc -c a.c"]}"#,
        &legacy_run_output("gcc -c a.c", 0)
    ));
    assert!(is_compile_command_success(
        r#"{"command":"","args":["gcc","-c","a.c"]}"#,
        &legacy_run_output("gcc -c a.c", 0)
    ));
    assert!(is_compile_command_success(
        r#"{"script":"cargo build"}"#,
        &legacy_run_output("cargo build", 0)
    ));
    assert!(!is_compile_command_success(
        r#"{"script":"cargo test"}"#,
        &legacy_run_output("cargo test", 0)
    ));
    assert!(!is_compile_command_success(
        r#"{"script":"cargo build"}"#,
        &legacy_run_output("cargo build", 1)
    ));
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
    assert!(names.contains(&"text_transform"));
    assert!(names.contains(&"cargo_check"));
    assert!(!names.contains(&"git_status"));
}

#[cfg(unix)]
#[tokio::test]
async fn run_tool_result_run_command_honors_command_timeout_secs() {
    let allowed = vec!["bash".into(), "sleep".into(), "echo".into()];
    let mut ctx = test_ctx(&allowed);
    ctx.command_timeout_secs = 1;
    let t0 = std::time::Instant::now();
    let result = run_tool_result_async(
        "run_command",
        r#"{"command":"bash","args":["-c","echo wf-partial; sleep 60"]}"#,
        &ctx,
    )
    .await;
    assert!(
        t0.elapsed() < std::time::Duration::from_secs(8),
        "dispatch must apply wall clock, elapsed={:?}",
        t0.elapsed()
    );
    assert!(!result.ok);
    assert_eq!(result.error_code.as_deref(), Some("timeout"));
    assert!(
        result.message.contains("wf-partial") || result.stdout.contains("wf-partial"),
        "message={} stdout={}",
        result.message,
        result.stdout
    );
}

#[test]
fn self_config_info_dumps_runtime_config_and_redacts_secrets() {
    let cfg = crate::config::load_config(None).expect("embed default config");
    let allowed = test_allowed_commands();
    let mut ctx = test_ctx(&allowed);
    ctx.cfg = Some(&cfg);
    let out = run_tool("self_config_info", "{}", &ctx);
    assert!(out.contains("[llm]"), "应含 llm 小节: {out}");
    assert!(out.contains("model ="), "应含 model 键: {out}");
    assert!(out.contains("api_base ="), "应含 api_base 键: {out}");
    assert!(out.contains("[web_search]"), "应含 web_search 小节: {out}");
    assert!(
        out.contains("api_key = 未设置") || out.contains("api_key = 已设置(值隐藏)"),
        "api_key 只允许报状态: {out}"
    );
    // `[tool_registry]` 须暴露后台任务配置（模型可自查 async 白名单与并发上限）。
    let tr = run_tool("self_config_info", r#"{"sections":["tool_registry"]}"#, &ctx);
    assert!(tr.contains("[tool_registry]"), "应含 tool_registry 小节: {tr}");
    assert!(tr.contains("background_jobs_enabled ="), "{tr}");
    assert!(tr.contains("background_job_async_tools = ["), "{tr}");
    assert!(tr.contains("background_job_max_concurrent ="), "{tr}");
    assert!(tr.contains("background_job_max_queued ="), "{tr}");
    assert!(tr.contains("background_job_ttl_secs ="), "{tr}");
    assert!(tr.contains("background_job_result_grace_secs ="), "{tr}");
    assert!(tr.contains("background_job_max_entries ="), "{tr}");
    assert!(tr.contains("background_job_output_buffer_bytes ="), "{tr}");
    let filtered = run_tool("self_config_info", r#"{"sections":["llm"]}"#, &ctx);
    assert!(filtered.contains("[llm]"));
    assert!(!filtered.contains("[sampling]"));
    assert!(
        run_tool("self_config_info", r#"{"sections":["bogus"]}"#, &ctx).contains("未匹配到任何配置小节")
    );
    assert!(run_tool("self_config_info", r#"{"sections":1}"#, &ctx).contains("参数与工具 JSON Schema 不一致"));
}

#[test]
fn builtin_legacy_runner_count_matches_baseline() {
    // 迁移 ratchet：每批迁移后把基线下调；禁止新增 Legacy 工具。
    // `archive-tools` 提供 3 个 Legacy 工具，基线随 feature 变化。
    // 本轮下调：`add_reminder` / `list_reminders` / `complete_reminder` / `delete_reminder` /
    // `update_reminder` / `add_event` / `list_events` / `delete_event` / `update_event`
    // 九个 Legacy 合并为 `reminder` + `event`（−7）。
    // 再下调：`get_current_time` / `calc` / `convert_units` 三个 Legacy 整体移除（−3）。
    // 本轮：`find_symbol` / `find_references` / `rust_file_outline` / `call_graph_sketch`
    // 四个只读导航 Legacy 合并为单个 `code_nav`（−3）。
    // 本轮：`misc_basic` 整族（`regex_test` / `date_calc` / `json_format` / `env_var_check`）
    // 迁为 Typed（−4）。
    // 本轮（B2 首段）：`text_diff` / `table_text` 迁为 Typed（−2）。
    // 本轮（B2 第二段）：`markdown_check_links` / `structured_validate` / `structured_query` /
    // `structured_diff` 迁为 Typed（−4）。
    // 本轮（B2 第三段）：`text_transform` / `structured_patch` 迁为 Typed（−2）。
    // 本轮（B4）：file_core 只读族 `read_dir` / `glob_files` / `list_tree` /
    // `read_binary_meta` / `hash_file` 迁为 Typed（−5）。
    // 本轮（A 档首项）：`self_config_info` 迁为 Typed（−1）—— 缺 `AgentConfig` 与非空
    // `sections` 未匹配均改为显式 `ToolError`。同项的 `skill_manage` 暂缓：其内部实现是
    // `Result<_, String>`（约 20 处 `"错误：…"`），需整体换成 `Result<_, ToolError>`，
    // 单独切片处理（见 `docs/design/tool_runner_typed_migration.md` §4.8）。
    const LEGACY_BASELINE: usize = if cfg!(feature = "archive-tools") { 43 } else { 40 };
    let legacy = tool_specs()
        .iter()
        .filter(|spec| matches!(spec.runner, ToolRunner::Legacy(_)))
        .count();
    assert_eq!(
        legacy, LEGACY_BASELINE,
        "ToolRunner::Legacy 数量应为 {LEGACY_BASELINE}，实际 {legacy}；迁移后请同步下调基线，新工具请直接使用 Typed"
    );
}

#[test]
fn typed_ok_is_success_even_if_body_looks_like_failure() {
    let (output, parsed) =
        finish_dispatch_typed_ok("demo_tool", "{}", "失败：示例正文\n超时 3s".to_string());
    assert!(parsed.ok, "Typed Ok 必须判定为成功");
    assert_eq!(parsed.error_code, None);
    // 正文启发式会误判失败，故输出须带显式 `ok` 头，供下游再次解析时保持一致。
    let reparsed = parse_legacy_output("demo_tool", &output);
    assert!(reparsed.ok, "下游再次解析仍应为成功: {output}");
    assert!(output.ends_with("失败：示例正文\n超时 3s"), "正文应保留: {output}");
}

#[test]
fn typed_outcome_output_only_prepends_header_on_mismatch() {
    let success_body = "正常输出".to_string();
    assert_eq!(
        typed_outcome_output("demo_tool", true, None, success_body.clone()),
        success_body,
        "启发式与真实成败一致时不得改写正文"
    );
    let failure_body = "错误：示例".to_string();
    assert_eq!(
        typed_outcome_output("demo_tool", false, None, failure_body.clone()),
        failure_body,
        "启发式与真实成败一致时不得改写正文"
    );

    let looks_ok_but_failed =
        typed_outcome_output("demo_tool", false, None, "无法解析路径".to_string());
    assert!(!parse_legacy_output("demo_tool", &looks_ok_but_failed).ok);
}

#[test]
fn typed_ok_end_to_end_survives_failure_like_file_body() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("failure_like.txt");
    std::fs::write(&path, "失败：示例正文\n超时 3s\n").expect("write");

    let allowed = test_allowed_commands();
    let ctx = ToolContext {
        working_dir: dir.path(),
        path_scope: crate::cm_types::PathScope::SessionRoot,
        ..test_ctx(&allowed)
    };
    let result = run_tool_result("read_file", r#"{"path":"failure_like.txt"}"#, &ctx);
    assert!(result.ok, "Typed Ok 必须为成功: {}", result.message);
    assert!(result.message.contains("失败：示例正文"));
}

#[test]
fn typed_error_without_failure_keywords_is_still_failed_on_string_surface() {
    // `read_file` 的空白 path 走到 `ToolError::invalid_args("缺少 path 参数")`，
    // 消息不含「错误 / 失败 / 超时」；主 Agent 路径只能拿到 String，须由显式 `ok` 头保证下游判失败。
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool("read_file", r#"{"path":"   "}"#, &ctx);
    assert!(out.contains("缺少 path 参数"), "{out}");
    let parsed = parse_legacy_output("read_file", &out);
    assert!(!parsed.ok, "无关键词的 Typed 失败必须判失败: {out}");
    assert!(parsed.error_code.is_some(), "失败时应能分类错误码: {out}");
}

// ── `misc_basic` 迁 Typed 后：错误码显式化，成功正文逐字节不变 ──

#[test]
fn misc_basic_typed_error_codes() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);

    // 原本靠正文「失败 / 错误」关键词判失败的输入：迁移后码从 `{tool}_failed` 精化为 `invalid_args`。
    let e = run_tool_try(
        "date_calc",
        r#"{"mode":"diff","from":"2024-13-01","to":"2024-01-31"}"#,
        &ctx,
    )
    .expect_err("非法 from 日期必须失败");
    assert_eq!(e.code, "invalid_args");
    assert_eq!(e.category, ToolFailureCategory::InvalidInput);

    let e = run_tool_try("regex_test", r#"{"pattern":"(","test_strings":["x"]}"#, &ctx)
        .expect_err("非法正则必须失败");
    assert_eq!(e.code, "regex_test_invalid_regex");
    assert_eq!(e.category, ToolFailureCategory::InvalidInput);
    assert!(e.message.contains("正则编译失败"), "{}", e.message);

    let e =
        run_tool_try("json_format", r#"{"text":"{oops"}"#, &ctx).expect_err("非法 JSON 必须失败");
    assert_eq!(e.code, "invalid_args");
    assert!(e.message.starts_with("JSON 解析失败"), "{}", e.message);

    let e = run_tool_try("env_var_check", r#"{"names":[]}"#, &ctx).expect_err("空 names 必须失败");
    assert_eq!(e.code, "invalid_args");
    // 钉住失败发生在 runner 层：`EnvVarCheckArgs.names` 目前没有 `minItems`；
    // 若将来加了，这条会先红，提示改断言而不是静默退化成「测 schema 层」。
    assert!(e.message.contains("names 不能为空"), "{}", e.message);
}

#[test]
fn misc_basic_typed_ok_keeps_body_verbatim() {
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);

    // 成功路径不注入 `crabmate_tool_output` 头，正文与迁移前逐字节一致。
    let out = run_tool("json_format", r#"{"text":"{\"a\":1}"}"#, &ctx);
    assert_eq!(out, "{\n  \"a\": 1\n}");

    let out = run_tool(
        "date_calc",
        r#"{"mode":"diff","from":"2024-01-01","to":"2024-01-31"}"#,
        &ctx,
    );
    assert!(out.starts_with("从 2024-01-01 到 2024-01-31："), "{out}");
    assert!(
        !out.contains("crabmate_tool_output"),
        "成功路径不得注入 ok 头: {out}"
    );

    let out = run_tool(
        "regex_test",
        r#"{"pattern":"\\d+","test_strings":["abc123"]}"#,
        &ctx,
    );
    assert!(out.contains("匹配: 1/1"), "{out}");
}

#[test]
fn misc_basic_typed_failure_reaches_string_surface_with_code() {
    // 主 Agent 走 `run_tool`（String 面）：真实错误码靠 `crabmate_tool_output` 头兜住，
    // 否则「JSON 解析失败」只会被关键词启发式归成 `json_format_failed`。
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool("json_format", r#"{"text":"{oops"}"#, &ctx);
    assert!(out.contains("JSON 解析失败"), "{out}");
    let parsed = parse_legacy_output("json_format", &out);
    assert!(!parsed.ok, "{out}");
    assert_eq!(parsed.error_code.as_deref(), Some("invalid_args"), "{out}");
}

#[test]
fn typed_parameter_error_without_keyword_reaches_string_surface_with_code() {
    // `text_diff` 缺 `right` 的正文是「inline 模式需要 right（字符串）」，一个关键词都不含 ——
    // 迁 Typed 之前它被 `parse_legacy_output` 判成 `ok: true`。`run_tool` 只返回 String，
    // 所以这条修复是否真落地，取决于 `typed_outcome_output` 是否注入了 `crabmate_tool_output` 头。
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool("text_diff", r#"{"mode":"inline","left":"a"}"#, &ctx);
    assert!(out.contains("inline 模式需要 right"), "{out}");
    let parsed = parse_legacy_output("text_diff", &out);
    assert!(!parsed.ok, "{out}");
    assert_eq!(parsed.error_code.as_deref(), Some("invalid_args"), "{out}");
}

#[test]
fn typed_ok_body_is_not_stamped_with_a_header() {
    // 成功正文与启发式一致时不注入头，保证既有正文逐字不变。
    let allowed = test_allowed_commands();
    let ctx = test_ctx(&allowed);
    let out = run_tool(
        "table_text",
        r#"{"action":"preview","text":"a,b\n1,2\n","preview_rows":2}"#,
        &ctx,
    );
    assert!(out.starts_with("delimiter="), "{out}");
    assert!(!out.contains("crabmate_tool_output"), "{out}");
}
