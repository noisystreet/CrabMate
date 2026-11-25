//! `summarize_tool_call` 的摘要形态测试（拆出以降低 `tests.rs` 物理行数）。

use super::*;

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
    let s = summarize_tool_call(
        "run_command",
        r#"{"command":["git"],"args":["status","-sb"]}"#,
    );
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
