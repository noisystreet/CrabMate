//! 工作区路径的语义化规范化（`..` / `.`）与根边界校验。
//!
//! 使用 [`path_absolutize::Absolutize`]，在**不依赖目标存在**的前提下解析路径；symlink 的最终落点仍由调用方对**已存在路径**做 `canonicalize` 校验。

use path_absolutize::Absolutize;
use std::path::{Path, PathBuf};

/// `sub` 必须为相对路径；在已 canonical 的 `workspace_root` 下解析并去掉 `.` / `..`，且不得越出根。
pub(crate) fn absolutize_relative_under_root(
    workspace_root: &Path,
    sub: &str,
) -> Result<PathBuf, String> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err("path 不能为空".to_string());
    }
    if Path::new(sub).is_absolute() {
        return Err("路径必须为相对于工作目录的相对路径，不能使用绝对路径".to_string());
    }
    let normalized = Path::new(sub)
        .absolutize_from(workspace_root)
        .map_err(|e| format!("路径规范化失败: {}", e))?;
    if !normalized.starts_with(workspace_root) {
        return Err("路径不能超出工作目录".to_string());
    }
    Ok(normalized.into_owned())
}

/// Web 工作区写入等：`sub` 可为绝对或相对路径；规范化后须落在 `base_canonical` 之下。
pub(crate) fn absolutize_workspace_subpath(
    base_canonical: &Path,
    sub: &str,
) -> Result<PathBuf, String> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err("path 不能为空".to_string());
    }
    let normalized = if Path::new(sub).is_absolute() {
        Path::new(sub)
            .absolutize()
            .map_err(|e| format!("路径规范化失败: {}", e))?
    } else {
        Path::new(sub)
            .absolutize_from(base_canonical)
            .map_err(|e| format!("路径规范化失败: {}", e))?
    };
    if !normalized.starts_with(base_canonical) {
        return Err("路径不能超出工作区根目录".to_string());
    }
    Ok(normalized.into_owned())
}
