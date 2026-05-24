use super::{tool_card_compact_text, tool_card_text};
use crate::ToolCardInput;
use crate::locale::ToolCardLocale;
use serde_json::json;

fn mk(summary: &str) -> ToolCardInput {
    ToolCardInput {
        name: "run_command".to_string(),
        goal_id: None,
        tool_call_id: None,
        result_version: 1,
        summary: Some(summary.to_string()),
        output: String::new(),
        ok: Some(true),
        exit_code: Some(0),
        error_code: None,
        failure_category: None,
        structured_preview: None,
    }
}

#[test]
fn compact_snake_tool_skips_pipe_when_summary_hyphen_cli_dup() {
    let mut info = mk("");
    info.name = "pre_commit_run".to_string();
    info.summary = Some("pre_commit_run\n\npre-commit run".to_string());
    let compact = tool_card_compact_text(&info, ToolCardLocale::ZhHans);
    assert!(
        !compact.contains('｜'),
        "不应拼出 snake 与 CLI 同义的两段: {compact:?}"
    );
    let detail = tool_card_text(&info, ToolCardLocale::ZhHans);
    assert!(
        !detail.contains("pre_commit_run\n\npre-commit run"),
        "详情不应叠两行同义: {detail:?}"
    );
}

#[test]
fn compact_git_diff_keeps_paren_suffix_without_dup_cli_head() {
    let mut info = mk("");
    info.name = "git_diff".to_string();
    info.summary =
        Some("git diff (working): frontend/src/message_format/tool_card/mod.rs".to_string());
    let out = tool_card_compact_text(&info, ToolCardLocale::ZhHans);
    assert!(out.contains('｜'), "应保留参数后缀: {out:?}");
    assert!(
        out.contains("(working): frontend/src/message_format/tool_card/mod.rs"),
        "compact={out:?}"
    );
    assert!(
        !out.contains("git diff (working)"),
        "不应重复整段 CLI 头: {out:?}"
    );
    assert!(out.starts_with("git_diff"), "compact={out:?}");
}

#[test]
fn terminal_session_success_detail_shows_command_and_capture() {
    let mut info = mk("✅ terminal_session 成功: terminal_session exec python3");
    info.name = "terminal_session".to_string();
    info.summary = Some("terminal_session exec python3 -c \"print('Hello, World!')\"".to_string());
    info.output = "Hello, World!\n".to_string();
    let out = tool_card_text(&info, ToolCardLocale::ZhHans);
    assert!(
        out.starts_with("$ python3 -c \"print('Hello, World!')\""),
        "unexpected detail head: {out:?}"
    );
    assert!(out.contains("Hello, World!"));
    assert!(!out.contains("交互终端"));
    assert!(!out.contains("terminal_session exec"));
}

#[test]
fn create_file_detail_shows_write_diff_and_strips_json_line_from_raw() {
    let mut info = mk("✅ create_file 成功");
    info.name = "create_file".to_string();
    let header = json!({
        "kind": "crabmate_tool_output",
        "tool": "create_file",
        "version": 1,
        "preview": "workspace_write_diff",
        "files": [{
            "path": "a.rs",
            "unified_diff": "--- a/a.rs\n+++ b/a.rs\n+fn x() {}\n",
            "truncated": false
        }],
        "preview_truncated": false
    });
    let line1 = serde_json::to_string(&header).unwrap();
    info.structured_preview = Some(header);
    info.output = format!("{line1}\n路径：a.rs\n已创建");
    let out = tool_card_text(&info, ToolCardLocale::ZhHans);
    assert!(out.contains("变更预览"));
    assert!(out.contains("```diff"));
    assert!(!out.contains("crabmate_tool_output"));
    assert!(out.contains("路径：a.rs"));
}

#[test]
fn run_command_success_detail_shell_title_skips_dup_cmd_line() {
    let mut info = mk("git diff --cached --stat");
    info.output =
        "命令：git diff --cached --stat\n退出码：0\n标准输出：\n  foo.rs | 1 +\n".to_string();
    let out = tool_card_text(&info, ToolCardLocale::ZhHans);
    assert!(
        out.starts_with("$ git diff --cached --stat"),
        "unexpected head: {out:?}"
    );
    assert!(
        !out.contains("命令执行"),
        "should not use generic title: {out}"
    );
    assert!(
        !out.contains("命令：git diff"),
        "should drop duplicate 命令 line: {out}"
    );
    assert!(out.contains("退出码：0"), "{out}");
    assert!(out.contains("foo.rs"), "{out}");
}

#[test]
fn git_status_success_detail_appends_raw_stdout() {
    let mut info = mk("✅ git_status 成功");
    info.name = "git_status".to_string();
    info.summary = Some("git status".to_string());
    info.output = "On branch main\nnothing to commit, working tree clean\n".to_string();
    let out = tool_card_text(&info, ToolCardLocale::ZhHans);
    assert!(out.contains("On branch main"), "{out}");
    assert!(out.contains("working tree clean"), "{out}");
}

#[test]
fn compact_git_status_skips_pipe_when_signal_is_cli_spelling_of_tool_id() {
    let mut info = mk("");
    info.name = "git_status".to_string();
    info.summary = Some("git status".to_string());
    let out = tool_card_compact_text(&info, ToolCardLocale::ZhHans);
    assert!(
        !out.contains("｜"),
        "不应拼出 tool_id 与摘要 CLI 的重复两段: {out:?}"
    );
    assert_eq!(out, "git_status");
}

#[test]
fn git_status_detail_skips_redundant_summary_body_before_raw() {
    let mut info = mk("");
    info.name = "git_status".to_string();
    info.summary = Some("git status".to_string());
    info.output = "On branch main\n".to_string();
    let out = tool_card_text(&info, ToolCardLocale::ZhHans);
    assert!(
        !out.contains("git_status\n\ngit status"),
        "详情不应叠两行同义: {out:?}"
    );
    assert!(out.contains("On branch main"), "{out}");
}

#[test]
fn rewrite_raw_success_summary_to_readable_text() {
    let s = "✅ run_command 成功: 退出码：0 标准输出： build CMakeLists.txt main.cpp";
    let out = tool_card_text(&mk(s), ToolCardLocale::ZhHans);
    assert!(out.starts_with("命令执行"));
    assert!(!out.starts_with("命令执行完成"));
    assert!(!out.contains("run_command 已完成"));
    assert!(out.contains("输出：build CMakeLists.txt main.cpp"));
    assert!(!out.contains("完成了什么"));
    assert!(!out.contains("已成功完成"));
    assert!(!out.contains("✅"));
}

#[test]
fn failed_tool_uses_standardized_sections() {
    let mut info = mk("❌ run_command 失败: 退出码：1 标准错误： permission denied");
    info.ok = Some(false);
    info.exit_code = Some(1);
    info.error_code = Some("command_failed".to_string());
    let out = tool_card_text(&info, ToolCardLocale::ZhHans);
    assert!(out.contains("发生了什么"));
    assert!(out.contains("影响范围"));
    assert!(out.contains("建议下一步"));
}

#[test]
fn failed_non_whitelist_tool_appends_full_output_block() {
    let mut info = mk("");
    // read_file 在跳过列表中：失败时走「完整输出」标题块，而非先整段拼接 output。
    info.name = "read_file".to_string();
    info.summary = Some("❌ read_file 失败: not found".to_string());
    info.ok = Some(false);
    info.output = "no such path: missing.rs".to_string();
    info.error_code = Some("not_found".to_string());
    let out = tool_card_text(&info, ToolCardLocale::ZhHans);
    assert!(
        out.contains("完整输出"),
        "expected heading in detail text: {out}"
    );
    assert!(out.contains("no such path: missing.rs"));
}

#[test]
fn cargo_check_success_detail_appends_raw_stdout() {
    let mut info = mk("✅ cargo_check 成功");
    info.name = "cargo_check".to_string();
    info.summary = Some("cargo check --message-format=short".to_string());
    info.output = "    Checking foo v0.1.0\n    Finished dev [unoptimized] target(s)\n".to_string();
    let out = tool_card_text(&info, ToolCardLocale::ZhHans);
    assert!(out.contains("Checking foo"), "{out}");
    assert!(out.contains("Finished dev"), "{out}");
}

#[test]
fn compact_text_stays_single_line_and_no_template_headers() {
    let s = "✅ run_command 成功: 退出码：0 标准输出： build CMakeLists.txt";
    let out = tool_card_compact_text(&mk(s), ToolCardLocale::ZhHans);
    assert!(!out.contains("完成了什么"));
    assert!(!out.contains('\n'));
    assert!(!out.contains("run_command 已完成"));
}

#[test]
fn compact_run_command_prefers_invocation_from_output() {
    let mut info = mk("cargo check");
    info.name = "run_command".to_string();
    info.output = "命令：cargo check --workspace\n退出码：0\n(无输出)\n".to_string();
    let out = tool_card_compact_text(&info, ToolCardLocale::ZhHans);
    assert!(out.contains("命令执行"));
    assert!(out.contains("cargo check --workspace"), "compact={out}");
    assert!(
        !out.contains("命令 ｜ cargo"),
        "不应重复「命令」标签: compact={out}"
    );
}

#[test]
fn compact_read_dir_uses_short_human_signal() {
    let mut info = mk("✅ read_dir 成功: 目录： . 总计遍历： 0，展示： 0");
    info.name = "read_dir".to_string();
    let out = tool_card_compact_text(&info, ToolCardLocale::ZhHans);
    assert!(out.contains("读取目录"));
    assert!(!out.contains("读取目录完成"));
    assert!(out.contains(". ｜ 0 项"));
    assert!(
        !out.contains("目录 . ｜"),
        "不应重复「目录」标签: compact={out}"
    );
}

#[test]
fn compact_read_file_uses_path_and_line_count() {
    let mut info = mk("✅ read_file 成功: 路径： src/main.cpp 行数： 128");
    info.name = "read_file".to_string();
    let out = tool_card_compact_text(&info, ToolCardLocale::ZhHans);
    assert!(out.contains("读取文件"));
    assert!(!out.contains("读取文件完成"));
    assert!(out.contains("src/main.cpp ｜ 128 行"));
}

#[test]
fn compact_read_file_parses_english_summary_line() {
    let mut info = mk("read file: src/lib.rs [1-10]");
    info.name = "read_file".to_string();
    let out = tool_card_compact_text(&info, ToolCardLocale::ZhHans);
    assert!(out.contains("读取文件"));
    assert!(
        out.contains("src/lib.rs"),
        "应展示路径而非占位「文件」: compact={out}"
    );
    assert!(!out.contains("读取文件 ｜ 文件 ｜"));
}

#[test]
fn compact_copy_file_shows_from_to_without_extra_label() {
    let mut info = mk("✅ copy_file 成功");
    info.name = "copy_file".to_string();
    info.output = "从→到：a.txt → b.txt\n已复制（12 字节）".to_string();
    let out = tool_card_compact_text(&info, ToolCardLocale::ZhHans);
    assert!(out.contains("a.txt → b.txt"), "compact 应含源→目标: {out}");
    assert!(!out.contains("从→到 ｜"), "不应再叠「从→到」标签: {out}");
}

#[test]
fn compact_read_file_prefers_json_header_in_output() {
    let hdr = r#"{"kind":"crabmate_tool_output","tool":"read_file","version":1,"path":"README.md","start_line":1,"end_line_shown":20,"line_count_returned":20,"total_lines":200,"truncated_by_max_lines":false,"has_more":true,"file_empty":false}"#;
    let mut info = mk("");
    info.name = "read_file".to_string();
    info.summary = Some("read file: README.md".to_string());
    info.output = format!("{hdr}\n文件: README.md\n总行数: 200\n...\n");
    let out = tool_card_compact_text(&info, ToolCardLocale::ZhHans);
    assert!(out.contains("README.md ｜ 200 行"), "compact={out}");
}

#[test]
fn compact_search_prefers_structured_output_with_scope_and_hits() {
    let hdr = r#"{"kind":"crabmate_tool_output","tool":"search_in_files","version":1,"pattern":"TODO","root":".","match_count":7,"files_visited":20,"max_results":200,"truncated":false}"#;
    let mut info = mk("✅ search_in_files 成功");
    info.name = "search_in_files".to_string();
    info.output =
        format!("{hdr}\n搜索：\"TODO\"\n范围：.\n匹配结果（最多 200 条，实际 7 条）：\n\n");
    let out = tool_card_compact_text(&info, ToolCardLocale::ZhHans);
    assert!(out.contains("全文检索"));
    assert!(!out.contains("全文检索完成"));
    assert!(
        out.contains("搜索 ｜ TODO · . · 命中 7 处"),
        "compact={out}"
    );
}

#[test]
fn compact_search_legacy_summary_fallback_keyword_and_hits() {
    let mut info = mk("✅ search_in_files 成功: 关键词： TODO 命中： 7");
    info.name = "search_in_files".to_string();
    let out = tool_card_compact_text(&info, ToolCardLocale::ZhHans);
    assert!(out.contains("全文检索"));
    assert!(out.contains("关键词 TODO ｜ 命中 7 处"));
}

#[test]
fn compact_strips_stream_placeholder_running_suffix_zh() {
    let mut info = mk("");
    info.name = "git_log".to_string();
    info.summary = Some("git log · 工具执行中…".to_string());
    let out = tool_card_compact_text(&info, ToolCardLocale::ZhHans);
    assert!(!out.contains("工具执行中"), "不应保留流式占位后缀: {out:?}");
}

#[test]
fn compact_strips_stream_placeholder_running_suffix_en() {
    let mut info = mk("");
    info.name = "git_log".to_string();
    info.summary = Some("git log · Running tools…".to_string());
    let out = tool_card_compact_text(&info, ToolCardLocale::En);
    assert!(
        !out.contains("Running tools"),
        "should not keep stream placeholder suffix: {out:?}"
    );
}
