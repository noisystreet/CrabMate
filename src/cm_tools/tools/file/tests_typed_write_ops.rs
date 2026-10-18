//! PR-1a / PR-1b Typed 迁移回归：`create_file` / `copy_file` / `move_file` / `modify_file`。
//!
//! 覆盖三类契约：成功正文与 String 面逐字节一致；失败返回显式 `code` / `category` / `retryable`；
//! 成功正文含「失败」「超时」等词时仍判成功（Typed 收尾注入 `ok=true` 头）。

use std::path::Path;

use super::test_support::test_ctx;
use super::tests::make_test_dir;
use super::{
    copy_file, copy_file_try, create_file, create_file_try, modify_file, modify_file_try, move_file,
    move_file_try,
};
use crate::cm_tools::tool_result::{ToolError, ToolFailureCategory};

fn assert_invalid_input(err: &ToolError) {
    assert_eq!(err.code, "invalid_args");
    assert_eq!(err.category, ToolFailureCategory::InvalidInput);
    assert!(!err.retryable);
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

#[test]
fn create_file_try_success_matches_string_face() {
    let dir_legacy = make_test_dir();
    let dir_typed = make_test_dir();
    let args = r#"{"path":"a.txt","content":"hello\nworld\n"}"#;

    let legacy = create_file(args, &dir_legacy, &test_ctx(&dir_legacy));
    let typed = create_file_try(args, &dir_typed, &test_ctx(&dir_typed)).expect("创建应成功");

    assert_eq!(legacy, typed, "成功正文必须与 String 面逐字节一致");
    assert!(typed.contains("\"ok\":true"), "{typed}");
    assert_eq!(read(&dir_typed, "a.txt"), "hello\nworld\n");

    let _ = std::fs::remove_dir_all(&dir_legacy);
    let _ = std::fs::remove_dir_all(&dir_typed);
}

#[test]
fn create_file_try_existing_path_is_invalid_input() {
    let dir = make_test_dir();
    std::fs::write(dir.join("a.txt"), "old").unwrap();

    let err = create_file_try(
        r#"{"path":"a.txt","content":"new"}"#,
        &dir,
        &test_ctx(&dir),
    )
    .expect_err("已存在路径须失败");

    assert_invalid_input(&err);
    assert!(err.message.contains("文件已存在"), "{}", err.message);
    assert_eq!(read(&dir, "a.txt"), "old");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn create_file_try_precheck_failure_keeps_legacy_body_and_code() {
    let dir = make_test_dir();

    let err = create_file_try(
        r#"{"path":"bad.json","content":"{not json"}"#,
        &dir,
        &test_ctx(&dir),
    )
    .expect_err("写盘前校验失败须拒绝写入");

    assert_eq!(err.code, "write_precheck_failed");
    assert_eq!(err.category, ToolFailureCategory::InvalidInput);
    assert!(!err.retryable);
    assert!(
        err.message.contains("校验失败（JSON_SYNTAX_ERROR）"),
        "{}",
        err.message
    );
    assert!(!dir.join("bad.json").exists(), "校验失败不得写盘");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn create_file_try_path_escape_is_workspace_resolve_failed() {
    let dir = make_test_dir();

    let err = create_file_try(
        r#"{"path":"../escape.txt","content":"x"}"#,
        &dir,
        &test_ctx(&dir),
    )
    .expect_err("越界路径须失败");

    assert_eq!(err.code, "workspace_resolve_failed");
    assert_eq!(err.category, ToolFailureCategory::External);
    assert!(!err.retryable);
    assert!(err.message.starts_with("错误："), "{}", err.message);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn copy_file_try_success_matches_string_face() {
    let dir_legacy = make_test_dir();
    let dir_typed = make_test_dir();
    std::fs::write(dir_legacy.join("a.txt"), "hello").unwrap();
    std::fs::write(dir_typed.join("a.txt"), "hello").unwrap();
    let args = r#"{"from":"a.txt","to":"sub/b.txt"}"#;

    let legacy = copy_file(args, &dir_legacy, &test_ctx(&dir_legacy));
    let typed = copy_file_try(args, &dir_typed, &test_ctx(&dir_typed)).expect("复制应成功");

    assert_eq!(legacy, typed, "成功正文必须与 String 面逐字节一致");
    assert!(typed.contains("\"ok\":true"), "{typed}");
    assert_eq!(read(&dir_typed, "sub/b.txt"), "hello");

    let _ = std::fs::remove_dir_all(&dir_legacy);
    let _ = std::fs::remove_dir_all(&dir_typed);
}

#[test]
fn copy_file_try_existing_without_overwrite_is_invalid_input() {
    let dir = make_test_dir();
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    std::fs::write(dir.join("b.txt"), "b").unwrap();

    let err = copy_file_try(
        r#"{"from":"a.txt","to":"b.txt"}"#,
        &dir,
        &test_ctx(&dir),
    )
    .expect_err("目标已存在且未 overwrite 须失败");

    assert_invalid_input(&err);
    assert!(err.message.contains("overwrite"), "{}", err.message);
    assert_eq!(read(&dir, "b.txt"), "b");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn copy_file_try_missing_from_is_invalid_input() {
    let dir = make_test_dir();

    let err = copy_file_try(r#"{"to":"b.txt"}"#, &dir, &test_ctx(&dir))
        .expect_err("缺少 from 须失败");

    assert_invalid_input(&err);
    assert!(err.message.contains("缺少 from"), "{}", err.message);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn move_file_try_success_matches_string_face() {
    let dir_legacy = make_test_dir();
    let dir_typed = make_test_dir();
    std::fs::write(dir_legacy.join("a.txt"), "mv").unwrap();
    std::fs::write(dir_typed.join("a.txt"), "mv").unwrap();
    let args = r#"{"from":"a.txt","to":"c.txt"}"#;

    let legacy = move_file(args, &dir_legacy, &test_ctx(&dir_legacy));
    let typed = move_file_try(args, &dir_typed, &test_ctx(&dir_typed)).expect("移动应成功");

    assert_eq!(legacy, typed, "成功正文必须与 String 面逐字节一致");
    assert!(!dir_typed.join("a.txt").exists());
    assert_eq!(read(&dir_typed, "c.txt"), "mv");

    let _ = std::fs::remove_dir_all(&dir_legacy);
    let _ = std::fs::remove_dir_all(&dir_typed);
}

#[test]
fn move_file_success_with_failure_word_in_path_stays_ok() {
    let dir = make_test_dir();
    std::fs::write(dir.join("a.txt"), "mv").unwrap();

    // 目标文件名含「失败」：Legacy 正文启发式会误判，Typed 收尾须注入 `ok=true`。
    let result = crate::cm_tools::tools::run_tool_result(
        "move_file",
        r#"{"from":"a.txt","to":"失败.txt"}"#,
        &test_ctx(&dir),
    );

    assert!(
        result.ok,
        "错误码 {:?} 正文 {}",
        result.error_code, result.message
    );
    assert_eq!(result.error_code, None);
    assert!(
        result.message.contains("\"ok\":true"),
        "应注入显式成功头: {}",
        result.message
    );
    assert!(result.message.contains("已移动"), "{}", result.message);
    assert_eq!(read(&dir, "失败.txt"), "mv");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_tool_failure_exposes_typed_error_code_to_legacy_parse() {
    let dir = make_test_dir();
    std::fs::write(dir.join("a.txt"), "old").unwrap();

    // 主 Agent 路径只返回 String，精确错误码须经显式头保留。
    let output = crate::cm_tools::tools::run_tool(
        "create_file",
        r#"{"path":"a.txt","content":"new"}"#,
        &test_ctx(&dir),
    );
    let parsed = crate::cm_tools::tool_result::parse_legacy_output("create_file", &output);
    assert!(!parsed.ok, "{output}");
    assert_eq!(parsed.error_code.as_deref(), Some("invalid_args"), "{output}");

    let output = crate::cm_tools::tools::run_tool(
        "copy_file",
        r#"{"from":"../escape.txt","to":"b.txt"}"#,
        &test_ctx(&dir),
    );
    let parsed = crate::cm_tools::tool_result::parse_legacy_output("copy_file", &output);
    assert!(!parsed.ok, "{output}");
    assert_eq!(
        parsed.error_code.as_deref(),
        Some("workspace_resolve_failed"),
        "{output}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn move_file_try_missing_source_is_workspace_resolve_failed() {
    let dir = make_test_dir();

    let err = move_file_try(
        r#"{"from":"missing.txt","to":"b.txt"}"#,
        &dir,
        &test_ctx(&dir),
    )
    .expect_err("源文件不存在须失败");

    assert_eq!(err.code, "workspace_resolve_failed");
    assert_eq!(err.category, ToolFailureCategory::External);
    assert!(!err.retryable);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn modify_file_try_success_matches_string_face() {
    let dir_legacy = make_test_dir();
    let dir_typed = make_test_dir();
    std::fs::write(dir_legacy.join("a.txt"), "L1\nL2\nL3\n").unwrap();
    std::fs::write(dir_typed.join("a.txt"), "L1\nL2\nL3\n").unwrap();
    let args = r#"{"path":"a.txt","mode":"replace_lines","start_line":2,"end_line":2,"content":"X"}"#;

    let legacy = modify_file(args, &dir_legacy, &test_ctx(&dir_legacy));
    let typed = modify_file_try(args, &dir_typed, &test_ctx(&dir_typed)).expect("替换应成功");

    assert_eq!(legacy, typed, "成功正文必须与 String 面逐字节一致");
    assert!(typed.contains("已按行替换"), "{typed}");
    assert!(typed.contains("\"ok\":true"), "{typed}");
    assert_eq!(read(&dir_typed, "a.txt"), "L1\nX\nL3\n");

    let _ = std::fs::remove_dir_all(&dir_legacy);
    let _ = std::fs::remove_dir_all(&dir_typed);
}

#[test]
fn modify_file_try_missing_path_is_invalid_input() {
    let dir = make_test_dir();

    let err = modify_file_try(r#"{"content":"x"}"#, &dir, &test_ctx(&dir))
        .expect_err("缺少 path 须失败");

    assert_invalid_input(&err);
    assert!(err.message.contains("缺少 path"), "{}", err.message);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn modify_file_try_non_file_path_is_invalid_input() {
    let dir = make_test_dir();
    std::fs::create_dir(dir.join("sub")).unwrap();

    let err = modify_file_try(r#"{"path":"sub","content":"x"}"#, &dir, &test_ctx(&dir))
        .expect_err("目录不可作为修改目标");

    assert_invalid_input(&err);
    assert!(
        err.message.contains("路径不是文件或不存在"),
        "{}",
        err.message
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn modify_file_try_stale_revision_keeps_body_and_code() {
    let dir = make_test_dir();
    std::fs::write(dir.join("a.txt"), "L1\nL2\n").unwrap();

    let err = modify_file_try(
        r#"{"path":"a.txt","mode":"replace_lines","start_line":1,"end_line":1,"content":"X","base_revision":"deadbeef"}"#,
        &dir,
        &test_ctx(&dir),
    )
    .expect_err("过期 base_revision 须拒写");

    assert_eq!(err.code, "stale_revision");
    // `stale_revision` 无 `_failed` 后缀，按现有错误码映射归入 `Unknown`（与 Legacy 推断一致）。
    assert_eq!(err.category, ToolFailureCategory::Unknown);
    assert!(!err.retryable);
    assert!(err.message.contains("错误[stale_revision]"), "{}", err.message);
    assert_eq!(read(&dir, "a.txt"), "L1\nL2\n", "被拒绝的写入不得改动文件");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn modify_file_try_full_overwrite_guard_keeps_preview_and_failure() {
    let dir = make_test_dir();
    let original = "a\n".repeat(200);
    std::fs::write(dir.join("big.txt"), &original).unwrap();

    let err = modify_file_try(
        r#"{"path":"big.txt","mode":"full","content":""}"#,
        &dir,
        &test_ctx(&dir),
    )
    .expect_err("高危整文件覆盖未确认须拒写");

    assert_eq!(err.code, "modify_file_failed");
    assert_eq!(err.category, ToolFailureCategory::External);
    assert!(err.message.contains("未写盘"), "{}", err.message);
    assert!(
        err.message.contains("workspace_write_diff"),
        "拒绝路径仍须返回 diff 预览: {}",
        err.message
    );
    assert_eq!(read(&dir, "big.txt"), original, "拒绝时不得写盘");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn modify_file_failure_exposes_typed_error_code_to_legacy_parse() {
    let dir = make_test_dir();
    std::fs::create_dir(dir.join("sub")).unwrap();

    // 主 Agent 路径只返回 String，精确错误码须经显式头保留。
    // Schema 校验可通过的目录路径：Legacy 正文会推断为 `modify_file_failed`，
    // Typed 面返回 `invalid_args`，以此验证头部确实覆盖了正文启发式。
    let output = crate::cm_tools::tools::run_tool(
        "modify_file",
        r#"{"path":"sub","content":"x"}"#,
        &test_ctx(&dir),
    );
    let parsed = crate::cm_tools::tool_result::parse_legacy_output("modify_file", &output);
    assert!(!parsed.ok, "{output}");
    assert_eq!(parsed.error_code.as_deref(), Some("invalid_args"), "{output}");

    let _ = std::fs::remove_dir_all(&dir);
}
