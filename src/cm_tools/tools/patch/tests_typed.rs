//! PR-1e Typed 迁移回归：`apply_patch`。
//!
//! 覆盖三类契约：成功正文与 String 面逐字节一致；失败返回显式 `code` / `category` / `retryable`；
//! 成功正文含「失败」等词时仍判成功（Typed 收尾注入 `ok=true` 头）。

use std::path::Path;

use super::{run_with_changelist, run_with_changelist_try};
use crate::cm_tools::tool_result::ToolFailureCategory;
use crate::cm_tools::tools::testing::{
    assert_invalid_input, assert_run_tool_error_code, assert_success_parity,
    assert_success_with_failure_wording, make_test_dir, test_ctx,
};

const PATCH_MODIFY: &str = r#"--- a.txt
+++ a.txt
@@ -1,1 +1,1 @@
-old
+new
"#;

fn args_with_patch(patch: &str) -> String {
    serde_json::json!({ "patch": patch }).to_string()
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

#[test]
#[allow(clippy::result_large_err)] // 闭包返回 `Result<String, ToolError>`
fn apply_patch_try_success_matches_string_face() {
    let dir_legacy = make_test_dir();
    let dir_typed = make_test_dir();
    std::fs::write(dir_legacy.join("a.txt"), "old\n").unwrap();
    std::fs::write(dir_typed.join("a.txt"), "old\n").unwrap();
    let args = args_with_patch(PATCH_MODIFY);

    let typed = assert_success_parity(
        &dir_legacy,
        &dir_typed,
        || run_with_changelist(&args, &test_ctx(&dir_legacy)),
        || run_with_changelist_try(&args, &test_ctx(&dir_typed)),
    );

    assert!(typed.contains("\"ok\":true"), "{typed}");
    assert!(typed.contains("补丁应用成功"), "{typed}");
    assert_eq!(read(&dir_typed, "a.txt"), "new\n");

    let _ = std::fs::remove_dir_all(&dir_legacy);
    let _ = std::fs::remove_dir_all(&dir_typed);
}

#[test]
fn apply_patch_try_missing_patch_is_invalid_input() {
    let dir = make_test_dir();

    for args in [r#"{}"#, r#"{"patch":"   "}"#] {
        let err = run_with_changelist_try(args, &test_ctx(&dir)).expect_err("缺少 patch 须失败");
        assert_invalid_input(&err);
        assert_eq!(err.message, "错误：缺少 patch 参数");
        // String 面必须逐字节保留历史文案。
        assert_eq!(run_with_changelist(args, &test_ctx(&dir)), err.message);
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn apply_patch_try_invalid_json_is_invalid_input() {
    let dir = make_test_dir();

    let err = run_with_changelist_try("{", &test_ctx(&dir)).expect_err("非法 JSON 须失败");
    assert_invalid_input(&err);
    assert!(
        err.message.starts_with("参数 JSON 无效:"),
        "{}",
        err.message
    );
    assert_eq!(run_with_changelist("{", &test_ctx(&dir)), err.message);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn apply_patch_try_rejects_escaping_paths_as_invalid_input() {
    let dir = make_test_dir();
    std::fs::write(dir.join("a.txt"), "old\n").unwrap();

    let cases = [
        "--- /etc/passwd\n+++ /etc/passwd\n@@ -1 +1 @@\n-a\n+b\n",
        "--- ../pwned.txt\n+++ ../pwned.txt\n@@ -1 +1 @@\n-a\n+b\n",
    ];
    for patch in cases {
        let args = args_with_patch(patch);
        let err = run_with_changelist_try(&args, &test_ctx(&dir)).expect_err("越界路径须失败");
        assert_invalid_input(&err);
        assert!(
            err.message.starts_with("补丁路径校验失败: "),
            "{}",
            err.message
        );
    }

    assert_eq!(read(&dir, "a.txt"), "old\n");
    assert!(!dir.join("pwned.txt").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn apply_patch_try_requires_unified_diff_header() {
    let dir = make_test_dir();

    let err = run_with_changelist_try(&args_with_patch("hello\n"), &test_ctx(&dir))
        .expect_err("无文件头须失败");
    assert_invalid_input(&err);
    assert_eq!(
        err.message,
        "补丁路径校验失败: 未检测到 unified diff 文件头（--- / +++）"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn apply_patch_try_malformed_diff_is_invalid_input() {
    let dir = make_test_dir();
    std::fs::write(dir.join("a.txt"), "old\n").unwrap();
    // hunk 正文每行必须以空格 / `-` / `+` / `\` 开头；`old` 缺前缀。
    let patch = "--- a.txt\n+++ a.txt\n@@ -1,1 +1,1 @@\nold\n+new\n";

    let err = run_with_changelist_try(&args_with_patch(patch), &test_ctx(&dir))
        .expect_err("非法 hunk 须失败");
    assert_invalid_input(&err);
    assert!(
        err.message.starts_with("解析 unified diff 失败: "),
        "{}",
        err.message
    );
    assert_eq!(read(&dir, "a.txt"), "old\n");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn apply_patch_try_all_failure_keeps_single_header() {
    let dir = make_test_dir();
    let patch = "--- missing.txt\n+++ missing.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n";
    let args = args_with_patch(patch);

    let err = run_with_changelist_try(&args, &test_ctx(&dir)).expect_err("目标不存在须失败");
    assert_eq!(err.code, "apply_patch_failed");
    assert_eq!(err.category, ToolFailureCategory::External);
    assert!(!err.retryable);
    assert!(err.message.contains("补丁应用失败"), "{}", err.message);
    assert_eq!(
        err.message.matches("crabmate_tool_output").count(),
        1,
        "失败正文只应有一个显式头: {}",
        err.message
    );
    assert_eq!(run_with_changelist(&args, &test_ctx(&dir)), err.message);

    let output = assert_run_tool_error_code(
        "apply_patch",
        &args,
        &test_ctx(&dir),
        "apply_patch_failed",
    );
    assert_eq!(output.matches("crabmate_tool_output").count(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

/// 两阶段提交：多文件补丁里任一文件对不上 → **整体不写**。
///
/// 旧实现边算边写，会把 `good.txt` 先改掉、只在文案里说「部分应用」。现在先全部试算，
/// 失败时一个字节都不落盘——这条测试钉住「不留半套修改」。
#[test]
fn apply_patch_try_partial_failure_writes_nothing() {
    let dir = make_test_dir();
    std::fs::write(dir.join("good.txt"), "old\n").unwrap();
    let patch = r#"--- good.txt
+++ good.txt
@@ -1,1 +1,1 @@
-old
+new
--- missing.txt
+++ missing.txt
@@ -1,1 +1,1 @@
-old
+new
"#;
    let args = args_with_patch(patch);

    let err = run_with_changelist_try(&args, &test_ctx(&dir)).expect_err("部分失败须失败");
    assert_eq!(err.code, "apply_patch_failed");
    assert_eq!(err.category, ToolFailureCategory::External);
    assert!(!err.retryable);
    assert!(err.message.contains("补丁应用失败"), "{}", err.message);
    assert_eq!(
        err.message.matches("crabmate_tool_output").count(),
        1,
        "失败正文只应有一个显式头: {}",
        err.message
    );
    // 关键断言：先试算后落盘 ⇒ 能应用的那个文件也**不得**被改动。
    assert_eq!(read(&dir, "good.txt"), "old\n", "部分失败不得留下半套修改");
    assert_eq!(run_with_changelist(&args, &test_ctx(&dir)), err.message);

    let output = assert_run_tool_error_code(
        "apply_patch",
        &args,
        &test_ctx(&dir),
        "apply_patch_failed",
    );
    assert_eq!(output.matches("crabmate_tool_output").count(), 1);
    assert_eq!(read(&dir, "good.txt"), "old\n");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn apply_patch_try_create_file_named_failure_still_succeeds() {
    let dir = make_test_dir();
    let patch = "--- /dev/null\n+++ 失败.txt\n@@ -0,0 +1,1 @@\n+hello\n";
    let args = args_with_patch(patch);
    let ctx = test_ctx(&dir);

    let output = assert_success_with_failure_wording("apply_patch", &args, &ctx);
    assert!(output.contains("\"ok\":true"), "{output}");
    assert_eq!(read(&dir, "失败.txt"), "hello\n");

    let _ = std::fs::remove_dir_all(&dir);
}
