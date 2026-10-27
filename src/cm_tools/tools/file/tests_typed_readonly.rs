//! file_core 只读族（`read_dir` / `glob_files` / `list_tree` / `read_binary_meta`）
//! Typed 迁移后的工作区路径回归钉。
//!
//! `resolve_for_read_open` 先 `canonicalize` 再判断是否落在根内，故 `OutsideWorkspaceRoot`
//! 只在越界目标**真实存在**时出现，目标不存在则落 `*_io_failed`。两类原因必须可分，否则
//! 模型会把「文件不存在」误读成工作区配置问题。历史这些分支并非静默成功（`错误：…` 首行
//! 本就判失败），问题在于统一塌缩成 `{tool}_failed` / `external`。

use std::path::Path;

use super::tests::make_test_dir;
use super::{PathAccess, read_binary_meta_try};
use crate::cm_tools::tool_result::ToolFailureCategory;

/// 在临时工作区的**父目录**造一个真实文件，返回文件名（供 `../<name>` 引用）。
fn write_outside_sibling(dir: &Path) -> String {
    let name = format!(
        "crabmate_outside_{}.dat",
        dir.file_name().and_then(|n| n.to_str()).unwrap_or("ws")
    );
    let outside = dir.parent().expect("临时工作区应有父目录").join(&name);
    std::fs::write(&outside, b"outside").expect("写入工作区外文件");
    name
}

#[test]
fn read_binary_meta_try_outside_workspace_is_workspace_resolve_failed() {
    let dir = make_test_dir();
    let name = write_outside_sibling(&dir);
    let args = format!(r#"{{"path":"../{name}"}}"#);
    let err = read_binary_meta_try(&args, &PathAccess::new(&dir)).expect_err("工作区外路径应显式失败");
    assert_eq!(err.code, "read_binary_meta_workspace_resolve_failed");
    assert_eq!(err.category, ToolFailureCategory::Workspace);
    assert_eq!(err.message, "错误：路径不能超出工作目录");
    let _ = std::fs::remove_file(dir.parent().expect("父目录").join(&name));
    let _ = std::fs::remove_dir_all(&dir);
}


#[test]
fn read_binary_meta_try_missing_file_is_io_failed() {
    let dir = make_test_dir();
    let err = read_binary_meta_try(r#"{"path":"missing.dat"}"#, &PathAccess::new(&dir))
        .expect_err("不存在的文件应显式失败");
    assert_eq!(err.code, "read_binary_meta_io_failed");
    assert_eq!(err.category, ToolFailureCategory::External);
    assert!(
        err.message.starts_with("错误：路径无法解析"),
        "{}",
        err.message
    );
    let _ = std::fs::remove_dir_all(&dir);
}
