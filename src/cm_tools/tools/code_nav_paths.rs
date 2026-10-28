//! `code_nav` 四个 action 共用的工作区路径解析与 IO 失败码。
//!
//! `symbol` / `code_nav` / `call_graph_sketch` 各自复制过一份
//! `resolve_root` / `resolve_file`（拒绝绝对路径、拒绝 `..`、canonicalize 后校验
//! 越界）。迁 `Typed` 时这是第三份，按既有约定（第三次出现即抽）收敛到这里：
//! 拒绝语义与 `error_code` 只在一处定义，三个 action 不再各自漂移。
//!
//! **迁移动机**：旧实现里这些拒绝分支以 `return e` 直接回正文，措辞以「路径」开头、
//! 不含「失败 / 超时」，`looks_like_failure` 判 `ok: true` —— 模型把「路径被拒」
//! 读成「工作区里没有这个符号 / 这个文件没有结构」。

use std::path::{Path, PathBuf};

use crate::cm_tools::tool_result::ToolError;

/// 绝对路径：工作区外的绝对路径一律拒绝（`PolicyDenied`）。
fn path_absolute_not_allowed() -> ToolError {
    ToolError::with_code(
        "code_nav_path_absolute_not_allowed",
        "错误：路径必须为工作区内相对路径，不能使用绝对路径".to_string(),
    )
}

/// `..` 分量：即便规范化后仍在工作区内也拒绝（与 `search_in_files` 口径一致）。
fn path_parent_not_allowed() -> ToolError {
    ToolError::with_code(
        "code_nav_path_parent_not_allowed",
        "错误：路径不能包含 ..".to_string(),
    )
}

/// 工作区根或目标路径 `canonicalize` 失败：换参数无用，属工作区环境问题。
fn workspace_resolve_failed(what: &str, e: &std::io::Error) -> ToolError {
    ToolError::workspace(
        "code_nav_workspace_resolve_failed",
        format!("错误：{what}无法解析: {e}"),
    )
}

/// 规范化后落在工作区根之外：安全拒绝。
fn path_outside_workspace(what: &str) -> ToolError {
    ToolError::with_code(
        "code_nav_path_outside_workspace",
        format!("错误：{what}不能超出工作区根目录"),
    )
}

/// 遍历 / 读取源文件失败。
///
/// 旧实现把 `搜索过程中发生错误：…` 写进正文，措辞不命中任何判定关键词，
/// IO 故障因此被读成「没有匹配结果」。
pub(super) fn search_io_failed(e: &str) -> ToolError {
    ToolError::external_code(
        "code_nav_search_failed",
        format!("错误：搜索过程中发生错误：{e}"),
    )
}

/// 解析搜索根：`sub` 为空时直接返回工作区根（**不做** `canonicalize`，保持旧行为）。
#[allow(clippy::result_large_err)] // `ToolError` 含正文解析快照（`parsed`），与 runner 契约一致
pub(super) fn resolve_search_root(base: &Path, sub: Option<&str>) -> Result<PathBuf, ToolError> {
    let Some(s) = sub else {
        return Ok(base.to_path_buf());
    };
    resolve_under(base, s, "搜索路径", "搜索路径")
}

/// 解析单个文件目标（`file_outline` 的 `path`）：解析与越界两侧的文案不同，故分开传。
#[allow(clippy::result_large_err)] // `ToolError` 含正文解析快照（`parsed`），与 runner 契约一致
pub(super) fn resolve_workspace_file(base: &Path, sub: &str) -> Result<PathBuf, ToolError> {
    resolve_under(base, sub, "目标文件路径", "目标路径")
}

#[allow(clippy::result_large_err)] // `ToolError` 含正文解析快照（`parsed`），与 runner 契约一致
fn resolve_under(
    base: &Path,
    sub: &str,
    resolve_label: &str,
    outside_label: &str,
) -> Result<PathBuf, ToolError> {
    let sub_path = Path::new(sub);
    if sub_path.is_absolute() {
        return Err(path_absolute_not_allowed());
    }
    if sub.contains("..") {
        return Err(path_parent_not_allowed());
    }
    let joined = base.join(sub_path);
    let canon_base = base
        .canonicalize()
        .map_err(|e| workspace_resolve_failed("工作区根目录", &e))?;
    let canon_joined = joined
        .canonicalize()
        .map_err(|e| workspace_resolve_failed(resolve_label, &e))?;
    if !canon_joined.starts_with(&canon_base) {
        return Err(path_outside_workspace(outside_label));
    }
    Ok(canon_joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "crabmate_code_nav_paths_{}_{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        root
    }

    /// 三个拒绝分支各自的码必须可区分：换参数能修的（绝对路径 / `..`）是 `PolicyDenied`，
    /// 换目录才能修的（工作区根不可解析）是 `Workspace`。
    #[test]
    fn rejects_absolute_parent_and_missing() {
        let root = temp_root("rejects");
        let abs = resolve_search_root(&root, Some("/etc")).expect_err("绝对路径应拒绝");
        assert_eq!(abs.code, "code_nav_path_absolute_not_allowed");

        let parent = resolve_search_root(&root, Some("../x")).expect_err("`..` 应拒绝");
        assert_eq!(parent.code, "code_nav_path_parent_not_allowed");

        let missing = resolve_workspace_file(&root, "nope.rs").expect_err("不存在应失败");
        assert_eq!(missing.code, "code_nav_workspace_resolve_failed");

        // 目录能 canonicalize，解析本身不判「是不是文件」——那条判定留给调用方。
        let dir = resolve_workspace_file(&root, "src").expect("目录本身可解析");
        assert!(dir.ends_with("src"), "{}", dir.display());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 无 `sub` 时直接返回工作区根，且**不** canonicalize（旧行为）。
    #[test]
    fn empty_sub_returns_root_verbatim() {
        let root = temp_root("verbs");
        let got = resolve_search_root(&root, None).expect("应成功");
        assert_eq!(got, root);
        let _ = std::fs::remove_dir_all(&root);
    }
}
