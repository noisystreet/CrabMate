//! PR-1d Typed 迁移回归：目录与权限类文件工具。
//!
//! 覆盖三类契约：成功正文与 String 面逐字节一致；失败返回显式 `code` / `category` / `retryable`；
//! 成功正文含「失败」等词时仍判成功（Typed 收尾注入 `ok=true` 头）。

use super::test_support::{make_test_dir, test_ctx, write};
use super::{
    create_dir, create_dir_try, delete_dir, delete_dir_try, delete_files, delete_files_try,
};
use crate::cm_tools::tool_result::ToolFailureCategory;
use crate::cm_tools::tools::testing::{
    assert_error, assert_invalid_input, assert_run_tool_error_code, assert_success_parity,
    assert_success_with_failure_wording,
};

// ── delete_files ────────────────────────────────────────────

#[test]
#[allow(clippy::result_large_err)] // 闭包返回 `Result<String, ToolError>`
fn delete_files_try_success_matches_string_face() {
    let dir_legacy = make_test_dir();
    let dir_typed = make_test_dir();
    for dir in [&dir_legacy, &dir_typed] {
        write(dir, "a.txt", "A");
        write(dir, "b.txt", "B");
    }
    let args = r#"{"paths":["a.txt","b.txt"],"confirm":true}"#;

    let typed = assert_success_parity(
        &dir_legacy,
        &dir_typed,
        || delete_files(args, &dir_legacy, &test_ctx(&dir_legacy)),
        || delete_files_try(args, &super::PathAccess::new(&dir_typed), &test_ctx(&dir_typed)),
    );

    assert!(typed.contains("已删除 2 个文件"), "{typed}");
    assert!(!dir_typed.join("a.txt").exists());
    assert!(!dir_typed.join("b.txt").exists());

    let _ = std::fs::remove_dir_all(&dir_legacy);
    let _ = std::fs::remove_dir_all(&dir_typed);
}

#[test]
fn delete_files_try_missing_confirm_is_invalid_input() {
    let dir = make_test_dir();
    write(&dir, "a.txt", "A");

    let err = delete_files_try(
        r#"{"paths":["a.txt"]}"#,
        &super::PathAccess::new(&dir),
        &test_ctx(&dir),
    )
    .expect_err("未确认的删除须失败");

    assert_invalid_input(&err);
    assert!(err.message.contains("confirm=true"), "{}", err.message);
    assert!(dir.join("a.txt").exists(), "被拒绝的删除不得改动文件");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_files_try_non_file_is_invalid_input() {
    let dir = make_test_dir();
    std::fs::create_dir(dir.join("sub")).unwrap();
    write(&dir, "sub/a.txt", "A");

    let err = delete_files_try(
        r#"{"paths":["sub"],"confirm":true}"#,
        &super::PathAccess::new(&dir),
        &test_ctx(&dir),
    )
    .expect_err("目录不能作为 delete_files 目标");

    // 正文含「错误」会被 Legacy 误判为 `delete_files_failed`，显式头须覆盖为 `invalid_args`。
    assert_run_tool_error_code(
        "delete_files",
        r#"{"paths":["sub"],"confirm":true}"#,
        &test_ctx(&dir),
        "invalid_args",
    );
    assert_invalid_input(&err);
    assert!(err.message.contains("不是文件"), "{}", err.message);
    assert!(dir.join("sub/a.txt").exists(), "整批拒绝不得部分删除");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_files_try_partial_failure_is_delete_files_failed() {
    // 同一文件用两种字面路径提交：校验阶段两者都通过，删除阶段第二次 `unlink` 必失败
    // （目标已不存在），因此不依赖目录权限，root 下也可稳定复现。
    let args = r#"{"paths":["a.txt","./a.txt"],"confirm":true}"#;
    let dir = make_test_dir();
    write(&dir, "a.txt", "A");

    let output = assert_run_tool_error_code(
        "delete_files",
        args,
        &test_ctx(&dir),
        "delete_files_failed",
    );
    // 失败正文已由写入 diff 预览头携带 `ok:false`；Typed 收尾不得再追加第二个头。
    assert_eq!(
        output.matches("crabmate_tool_output").count(),
        1,
        "{output}"
    );
    assert!(!dir.join("a.txt").exists());

    write(&dir, "b.txt", "B");
    let err = delete_files_try(
        r#"{"paths":["b.txt","./b.txt"],"confirm":true}"#,
        &super::PathAccess::new(&dir),
        &test_ctx(&dir),
    )
    .expect_err("重复路径的第二次删除须失败");

    assert_error(
        &err,
        "delete_files_failed",
        ToolFailureCategory::External,
        false,
    );
    assert!(err.message.contains("删除失败"), "{}", err.message);
    assert!(!dir.join("b.txt").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_files_success_with_failure_wording_stays_ok() {
    let dir = make_test_dir();
    write(&dir, "a失败.txt", "A");

    assert_success_with_failure_wording(
        "delete_files",
        r#"{"paths":["a失败.txt"],"confirm":true}"#,
        &test_ctx(&dir),
    );
    assert!(!dir.join("a失败.txt").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

// ── delete_dir ──────────────────────────────────────────────

#[test]
#[allow(clippy::result_large_err)] // 闭包返回 `Result<String, ToolError>`
fn delete_dir_try_success_matches_string_face() {
    let dir_legacy = make_test_dir();
    let dir_typed = make_test_dir();
    for dir in [&dir_legacy, &dir_typed] {
        write(dir, "sub/a.txt", "A");
    }
    let args = r#"{"path":"sub","recursive":true,"confirm":true}"#;

    let typed = assert_success_parity(
        &dir_legacy,
        &dir_typed,
        || delete_dir(args, &dir_legacy),
        || delete_dir_try(args, &super::PathAccess::new(&dir_typed)),
    );

    assert!(typed.contains("已删除目录（递归）"), "{typed}");
    assert!(!dir_typed.join("sub").exists());

    let _ = std::fs::remove_dir_all(&dir_legacy);
    let _ = std::fs::remove_dir_all(&dir_typed);
}

#[test]
fn delete_dir_try_non_empty_without_recursive_is_delete_dir_failed() {
    let dir = make_test_dir();
    write(&dir, "sub/a.txt", "A");

    let err = delete_dir_try(
        r#"{"path":"sub","confirm":true}"#,
        &super::PathAccess::new(&dir),
    )
        .expect_err("非空目录未递归须失败");

    assert_error(
        &err,
        "delete_dir_failed",
        ToolFailureCategory::External,
        false,
    );
    assert!(err.message.contains("目录非空"), "{}", err.message);
    assert!(dir.join("sub/a.txt").exists(), "失败时不得删除内容");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_dir_try_missing_path_is_invalid_input() {
    let dir = make_test_dir();

    let err = delete_dir_try(
        r#"{"path":"  ","confirm":true}"#,
        &super::PathAccess::new(&dir),
    )
        .expect_err("空 path 须失败");
    assert_invalid_input(&err);
    assert!(err.message.contains("缺少 path 参数"), "{}", err.message);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_dir_try_missing_confirm_is_invalid_input() {
    let dir = make_test_dir();
    std::fs::create_dir(dir.join("sub")).unwrap();

    // 正文不含失败关键词，Legacy 会误判成功；须由显式头覆盖为 `invalid_args`。
    assert_run_tool_error_code(
        "delete_dir",
        r#"{"path":"sub"}"#,
        &test_ctx(&dir),
        "invalid_args",
    );
    let err = delete_dir_try(r#"{"path":"sub"}"#, &super::PathAccess::new(&dir))
        .expect_err("缺 confirm 须失败");
    assert_invalid_input(&err);
    assert!(err.message.contains("confirm=true"), "{}", err.message);
    assert!(dir.join("sub").is_dir(), "失败时不得删除目录");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_dir_try_non_directory_is_invalid_input() {
    let dir = make_test_dir();
    write(&dir, "a.txt", "A");

    let err = delete_dir_try(
        r#"{"path":"a.txt","confirm":true}"#,
        &super::PathAccess::new(&dir),
    )
        .expect_err("文件路径不可当目录删除");
    assert_invalid_input(&err);
    assert!(err.message.contains("不是目录"), "{}", err.message);
    assert!(dir.join("a.txt").exists());

    // 「错误：… 不是目录」会被 Legacy 归为 `delete_dir_failed`，显式头须覆盖为 `invalid_args`。
    assert_run_tool_error_code(
        "delete_dir",
        r#"{"path":"a.txt","confirm":true}"#,
        &test_ctx(&dir),
        "invalid_args",
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn delete_dir_try_workspace_root_is_invalid_input() {
    let dir = make_test_dir();

    let err = delete_dir_try(
        r#"{"path":".","confirm":true}"#,
        &super::PathAccess::new(&dir),
    )
        .expect_err("不得删除工作区根目录");
    assert_invalid_input(&err);
    assert!(err.message.contains("工作区根目录"), "{}", err.message);
    assert!(dir.is_dir(), "工作区根目录必须保留");

    assert_run_tool_error_code(
        "delete_dir",
        r#"{"path":".","confirm":true}"#,
        &test_ctx(&dir),
        "invalid_args",
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_dir_success_with_failure_wording_stays_ok() {
    let dir = make_test_dir();
    std::fs::create_dir(dir.join("失败目录")).unwrap();

    assert_success_with_failure_wording(
        "delete_dir",
        r#"{"path":"失败目录","confirm":true}"#,
        &test_ctx(&dir),
    );
    assert!(!dir.join("失败目录").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

// ── create_dir ──────────────────────────────────────────────

#[test]
#[allow(clippy::result_large_err)] // 闭包返回 `Result<String, ToolError>`
fn create_dir_try_success_matches_string_face() {
    let dir_legacy = make_test_dir();
    let dir_typed = make_test_dir();
    let args = r#"{"path":"a/b"}"#;

    let typed = assert_success_parity(
        &dir_legacy,
        &dir_typed,
        || create_dir(args, &dir_legacy),
        || create_dir_try(args, &super::PathAccess::new(&dir_typed)),
    );

    assert!(typed.contains("已创建目录"), "{typed}");
    assert!(dir_typed.join("a/b").is_dir());

    let _ = std::fs::remove_dir_all(&dir_legacy);
    let _ = std::fs::remove_dir_all(&dir_typed);
}

#[test]
fn create_dir_try_existing_file_is_invalid_input() {
    let dir = make_test_dir();
    write(&dir, "a.txt", "A");

    let err = create_dir_try(r#"{"path":"a.txt"}"#, &super::PathAccess::new(&dir))
        .expect_err("已存在文件不可当目录创建");

    // 正文含「错误」会被 Legacy 误判为 `create_dir_failed`，显式头须覆盖为 `invalid_args`。
    assert_run_tool_error_code(
        "create_dir",
        r#"{"path":"a.txt"}"#,
        &test_ctx(&dir),
        "invalid_args",
    );
    assert_invalid_input(&err);
    assert!(err.message.contains("已存在且为文件"), "{}", err.message);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn create_dir_success_with_failure_wording_stays_ok() {
    let dir = make_test_dir();

    assert_success_with_failure_wording("create_dir", r#"{"path":"失败"}"#, &test_ctx(&dir));
    assert!(dir.join("失败").is_dir());

    let _ = std::fs::remove_dir_all(&dir);
}
