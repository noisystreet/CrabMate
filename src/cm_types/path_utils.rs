//! 工作区路径的语义化规范化与根边界校验（`crabmate-tools::workspace::path` 同源精简版）。
//!
//! 从 `crabmate-tools` 提取，供 `crabmate-memory` 等轻量 crate 使用，无需引入完整工具依赖。

use std::path::{Component, Path, PathBuf};
use thiserror::Error;

/// 工作区路径解析与策略校验失败。
#[derive(Debug, Error)]
pub enum WorkspacePathError {
    #[error("path 不能为空")]
    EmptyPath,
    #[error("路径必须为相对于工作目录的相对路径，不能使用绝对路径")]
    AbsolutePathNotAllowed,
    #[error("工作区根路径无法解析: {0}")]
    WorkspaceResolveFailed(#[source] std::io::Error),
    #[error("路径越出工作区根")]
    OutsideWorkspaceRoot,
}

impl WorkspacePathError {
    /// 用户可读的简短错误说明（当前等于 `Display`）。
    #[must_use]
    pub fn user_message(&self) -> String {
        self.to_string()
    }
}

/// 将工作目录（可为符号链接）解析为 **canonical** 绝对路径。
pub fn canonical_workspace_root(base: &Path) -> Result<PathBuf, WorkspacePathError> {
    base.canonicalize()
        .map_err(WorkspacePathError::WorkspaceResolveFailed)
}

/// `sub` 必须为相对路径；在已 canonical 的 `workspace_root` 下解析并去掉 `.` / `..`，且不得越出根。
pub fn absolutize_relative_under_root(
    workspace_root: &Path,
    sub: &str,
) -> Result<PathBuf, WorkspacePathError> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err(WorkspacePathError::EmptyPath);
    }
    if Path::new(sub).is_absolute() {
        return Err(WorkspacePathError::AbsolutePathNotAllowed);
    }
    let joined = workspace_root.join(sub);
    let normalized = normalize_path(&joined);
    if !normalized.starts_with(workspace_root) {
        return Err(WorkspacePathError::OutsideWorkspaceRoot);
    }
    Ok(normalized)
}

/// 解析 `.` / `..` 分量后返回规范化路径（不进行文件系统调用）。
fn normalize_path(path: &Path) -> PathBuf {
    let mut components: Vec<std::path::Component> = Vec::new();
    for c in path.components() {
        match c {
            Component::RootDir => components.push(c),
            Component::Normal(_) => components.push(c),
            Component::ParentDir => {
                components.pop();
            }
            _ => {}
        }
    }
    let mut result = PathBuf::new();
    for c in &components {
        result.push(c);
    }
    result
}
