//! `apply_patch` 写侧行为：`dry_run` 预览、单文件 CAS、多文件拒绝 `base_revision`。
//!
//! 语义说明：
//! - `dry_run` 默认 `false`（保持「应用」语义），显式 `true` 才只预览；
//! - 多文件补丁**总是**先整体试算、全部能应用才写（见 `tests_typed.rs` 的
//!   `partial_failure_writes_nothing`）；
//! - `base_revision` 只对**单文件**补丁有意义：一个 revision 无法表达多个文件的基线。

use std::path::Path;

use super::run_with_changelist_try;
use crate::cm_tools::tools::testing::{assert_invalid_input, make_test_dir, test_ctx};

const PATCH_MODIFY: &str = "--- a.txt\n+++ a.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n";

const PATCH_TWO_FILES: &str = "--- a.txt\n+++ a.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n--- b.txt\n+++ b.txt\n@@ -1,1 +1,1 @@\n-x\n+y\n";

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

fn seed(dir: &Path) {
    std::fs::write(dir.join("a.txt"), "old\n").unwrap();
    std::fs::write(dir.join("b.txt"), "x\n").unwrap();
}

#[test]
fn dry_run_previews_without_writing() {
    let dir = make_test_dir();
    seed(&dir);

    let args = serde_json::json!({ "patch": PATCH_MODIFY, "dry_run": true }).to_string();
    let out = run_with_changelist_try(&args, &test_ctx(&dir)).expect("预览应成功");

    assert_eq!(read(&dir, "a.txt"), "old\n", "dry_run 不得写盘");
    assert!(out.contains("a.txt"), "{out}");
    assert!(out.contains("-old"), "预览应给出 diff 正文: {out}");
    assert!(out.contains("+new"), "预览应给出 diff 正文: {out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dry_run_delete_previews_without_removing() {
    let dir = make_test_dir();
    std::fs::write(dir.join("gone.txt"), "bye\n").unwrap();

    let patch = "--- gone.txt\n+++ /dev/null\n@@ -1,1 +0,0 @@\n-bye\n";
    let args = serde_json::json!({ "patch": patch, "dry_run": true }).to_string();
    run_with_changelist_try(&args, &test_ctx(&dir)).expect("预览应成功");

    assert!(
        dir.join("gone.txt").exists(),
        "dry_run 不得删除文件"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stale_base_revision_rejects_single_file_patch() {
    let dir = make_test_dir();
    seed(&dir);

    let args = serde_json::json!({
        "patch": PATCH_MODIFY,
        "base_revision": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
    })
    .to_string();
    let err = run_with_changelist_try(&args, &test_ctx(&dir)).expect_err("过期基线须拒写");

    assert!(err.message.contains("stale_revision"), "{}", err.message);
    assert_eq!(read(&dir, "a.txt"), "old\n", "拒写时不得改动文件");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn base_revision_rejected_for_multi_file_patch() {
    let dir = make_test_dir();
    seed(&dir);

    // 一个 revision 无法表达两个文件的基线：明确拒绝，而不是静默忽略（那会让人以为有保护）。
    let args = serde_json::json!({
        "patch": PATCH_TWO_FILES,
        "base_revision": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
    })
    .to_string();
    let err = run_with_changelist_try(&args, &test_ctx(&dir)).expect_err("多文件须拒绝");

    assert_invalid_input(&err);
    assert!(
        err.message.contains("只能用于单文件补丁"),
        "{}",
        err.message
    );
    assert_eq!(read(&dir, "a.txt"), "old\n");
    assert_eq!(read(&dir, "b.txt"), "x\n");

    let _ = std::fs::remove_dir_all(&dir);
}
