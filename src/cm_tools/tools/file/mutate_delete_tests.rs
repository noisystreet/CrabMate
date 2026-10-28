//! `mutate::delete_files` 的测试（从 `mutate.rs` 拆出，保持文件行数上限）。

use super::delete_files;
use super::test_support::{test_ctx, write};
use std::fs;

#[test]
fn delete_files_batch_success_dedup() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "a.txt", "A");
    write(tmp.path(), "b.txt", "B");
    let ctx = test_ctx(tmp.path());
    let out = delete_files(
        r#"{"paths":["a.txt","b.txt","a.txt"],"confirm":true}"#,
        tmp.path(),
        &ctx,
    );
    assert!(out.contains("已删除 2 个文件"), "{out}");
    assert!(!tmp.path().join("a.txt").exists());
    assert!(!tmp.path().join("b.txt").exists());
}

#[test]
fn delete_files_requires_confirm() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "a.txt", "A");
    let ctx = test_ctx(tmp.path());
    let out = delete_files(r#"{"paths":["a.txt"]}"#, tmp.path(), &ctx);
    assert!(out.contains("需要 confirm=true"), "{out}");
    assert!(tmp.path().join("a.txt").exists());
}

#[test]
fn delete_files_validation_rejects_whole_batch() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "a.txt", "A");
    let ctx = test_ctx(tmp.path());
    let out = delete_files(
        r#"{"paths":["a.txt","missing.txt"],"confirm":true}"#,
        tmp.path(),
        &ctx,
    );
    assert!(!out.contains("已删除"), "{out}");
    assert!(out.contains("路径无法解析") || out.contains("错误"), "{out}");
    assert!(
        tmp.path().join("a.txt").exists(),
        "任一路径非法时整批拒绝，不应删除任何文件"
    );
}

#[test]
fn delete_files_rejects_directory() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("d")).unwrap();
    let ctx = test_ctx(tmp.path());
    let out = delete_files(r#"{"paths":["d"],"confirm":true}"#, tmp.path(), &ctx);
    assert!(out.contains("不是文件"), "{out}");
    assert!(tmp.path().join("d").is_dir());
}

#[test]
fn delete_files_caps_batch_size() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = test_ctx(tmp.path());
    let paths: Vec<String> = (0..33).map(|i| format!("f{i}.txt")).collect();
    let args = serde_json::json!({ "paths": paths, "confirm": true }).to_string();
    let out = delete_files(&args, tmp.path(), &ctx);
    assert!(out.contains("最多删除 32"), "{out}");
}

#[test]
fn delete_files_empty_paths_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = test_ctx(tmp.path());
    let out = delete_files(r#"{"paths":[],"confirm":true}"#, tmp.path(), &ctx);
    assert!(out.contains("缺少 paths"), "{out}");
}

/// 删除阶段遇个别失败（只读目录内的文件）时继续删完并汇总。
#[cfg(unix)]
#[test]
fn delete_files_partial_failure_continues() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "a.txt", "A");
    fs::create_dir(tmp.path().join("ro")).unwrap();
    write(tmp.path(), "ro/x.txt", "X");
    fs::set_permissions(tmp.path().join("ro"), fs::Permissions::from_mode(0o555)).unwrap();
    let ctx = test_ctx(tmp.path());
    let out = delete_files(
        r#"{"paths":["a.txt","ro/x.txt"],"confirm":true}"#,
        tmp.path(),
        &ctx,
    );
    fs::set_permissions(tmp.path().join("ro"), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(out.contains("已删除 1 个文件"), "{out}");
    assert!(out.contains("删除失败（1 个）"), "{out}");
    assert!(!tmp.path().join("a.txt").exists());
    assert!(tmp.path().join("ro/x.txt").exists());
    let r = crate::cm_tools::tool_result::ToolResult::from_output("delete_files", out);
    assert!(!r.ok, "删除阶段有失败项时应判失败");
    assert_eq!(r.error_code.as_deref(), Some("delete_files_failed"));
}

/// 全部删除失败时没有 diff 可预览，仍须显式声明 `ok=false`，避免正文首行「已删除 0 个文件」被误判成功。
#[cfg(unix)]
#[test]
fn delete_files_all_failures_report_failure() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("ro")).unwrap();
    write(tmp.path(), "ro/x.txt", "X");
    write(tmp.path(), "ro/y.txt", "Y");
    fs::set_permissions(tmp.path().join("ro"), fs::Permissions::from_mode(0o555)).unwrap();
    let ctx = test_ctx(tmp.path());
    let out = delete_files(
        r#"{"paths":["ro/x.txt","ro/y.txt"],"confirm":true}"#,
        tmp.path(),
        &ctx,
    );
    fs::set_permissions(tmp.path().join("ro"), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(out.contains("已删除 0 个文件"), "{out}");
    assert!(out.contains("删除失败（2 个）"), "{out}");
    assert!(tmp.path().join("ro/x.txt").exists());
    assert!(tmp.path().join("ro/y.txt").exists());
    let r = crate::cm_tools::tool_result::ToolResult::from_output("delete_files", out);
    assert!(!r.ok, "全部删除失败不得判成功");
    assert_eq!(r.error_code.as_deref(), Some("delete_files_failed"));
}
