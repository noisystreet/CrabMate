//! 工作区路径解析与校验（`file` 工具子模块）。
//!
//! 边界语义与 [`crate::path_workspace`] 一致。读路径优先经 [`resolve_for_read_open`]：Linux 上在已打开的工作区根 fd 上使用 **`openat2` + `RESOLVE_IN_ROOT`** 打开，将解析约束在根内并避免「校验后再次按路径 `open`」的窗口。其余局限见 [`crate::path_workspace`] 与 [`crate::workspace_fs`]。
#![allow(clippy::manual_string_new)]

use std::path::{Path, PathBuf};

use crate::path_workspace::{
    WorkspacePathError, absolutize_relative_under_root, ensure_canonical_within_root,
    ensure_existing_ancestor_within_root,
};
use crate::workspace_fs::OpenedWorkspaceFile;

pub(crate) use crate::path_workspace::canonical_workspace_root;

/// 将 [`WorkspacePathError`] 格式化为工具返回给模型的前缀文案（与历史 `错误：…` 一致）。
#[must_use]
pub(crate) fn tool_user_error_from_workspace_path(e: WorkspacePathError) -> String {
    format!("错误：{}", e.user_message())
}

/// 若 `sub` 为**已落在工作区根下**的绝对路径，则转为工作区相对路径；否则原样返回（并 trim）。  
/// 供 `read`/`write` 共用，减少模型误传 `/home/.../proj/foo` 时与「仅相对路径」规则冲突。
pub(crate) fn normalize_subpath_for_workspace(
    working_dir: &Path,
    sub: &str,
) -> Result<String, WorkspacePathError> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err(WorkspacePathError::EmptyPath);
    }
    if !Path::new(sub).is_absolute() {
        return Ok(sub.to_string());
    }
    let base = canonical_workspace_root(working_dir)?;
    let p = Path::new(sub);
    // 已存在时直接 canonical；若目标或中间目录尚不存在，则**向上**找到可 canonical 的已存在祖先，再拼上剩余相对后缀。
    let mut try_path: &Path = p;
    let resolved: std::path::PathBuf = loop {
        match try_path.canonicalize() {
            Ok(anchor) => {
                let rel_suffix = p.strip_prefix(try_path).map_err(|_| {
                    WorkspacePathError::PathResolveFailed(std::io::Error::other(
                        "absolute path prefix mismatch during workspace normalization",
                    ))
                })?;
                break anchor.join(rel_suffix);
            }
            Err(e) => {
                try_path = try_path
                    .parent()
                    .ok_or(WorkspacePathError::PathResolveFailed(e))?;
            }
        }
    };
    ensure_canonical_within_root(&resolved, &base)?;
    let rel = resolved
        .strip_prefix(&base)
        .map_err(|_| WorkspacePathError::OutsideWorkspaceRoot)?;
    let s = rel.to_string_lossy().replace('\\', "/");
    Ok(if s.is_empty() { ".".to_string() } else { s })
}

/// 解析用于读取或修改的路径（目标必须存在；path 必须为相对工作目录的相对路径）
pub(crate) fn resolve_for_read(base: &Path, sub: &str) -> Result<PathBuf, WorkspacePathError> {
    Ok(resolve_for_read_open(base, sub)?.resolved_path)
}

/// 与 [`resolve_for_read`] 相同策略校验，但用 **`openat2` / `RESOLVE_IN_ROOT`（Linux）** 或单次 `File::open` 打开，返回的 `metadata` 与 `file` 对应同一打开，缓解校验后二次按路径 `open` 的 TOCTOU。
pub(crate) fn resolve_for_read_open(
    base: &Path,
    sub: &str,
) -> Result<OpenedWorkspaceFile, WorkspacePathError> {
    let sub = normalize_subpath_for_workspace(base, sub)?;
    let sub = sub.trim();
    if sub.is_empty() {
        return Err(WorkspacePathError::EmptyPath);
    }
    if Path::new(sub).is_absolute() {
        return Err(WorkspacePathError::AbsolutePathNotAllowed);
    }
    let base_canonical = canonical_workspace_root(base)?;
    let joined = base_canonical.join(sub);
    let canonical = joined
        .canonicalize()
        .map_err(WorkspacePathError::PathResolveFailed)?;
    ensure_canonical_within_root(&canonical, &base_canonical)?;
    crate::workspace_fs::open_existing_file_under_root(&base_canonical, &canonical).map_err(|e| {
        WorkspacePathError::PathResolveFailed(std::io::Error::new(
            e.kind(),
            format!("open under workspace root: {e}"),
        ))
    })
}

/// 解析用于写入的路径（目标可不存在；path 必须为相对工作目录的相对路径，且不能通过 .. 超出工作目录）
pub(super) fn resolve_for_write(base: &Path, sub: &str) -> Result<PathBuf, WorkspacePathError> {
    let sub = normalize_subpath_for_workspace(base, sub)?;
    let sub = sub.trim();
    if sub.is_empty() {
        return Err(WorkspacePathError::EmptyPath);
    }
    if Path::new(sub).is_absolute() {
        return Err(WorkspacePathError::AbsolutePathNotAllowed);
    }
    let base_canonical = canonical_workspace_root(base)?;
    let normalized = absolutize_relative_under_root(&base_canonical, sub)?;
    ensure_existing_ancestor_within_root(&base_canonical, &normalized)?;
    Ok(normalized)
}

/// 相对工作区根的 POSIX 风格路径（供工具返回给模型，不暴露绝对路径）。
fn path_relative_to_workspace(working_dir: &Path, absolute: &Path) -> String {
    let Ok(base) = canonical_workspace_root(working_dir) else {
        return absolute.display().to_string();
    };
    match absolute.strip_prefix(&base) {
        Ok(rel) => {
            let s = rel.to_string_lossy().replace('\\', "/");
            if s.is_empty() { ".".to_string() } else { s }
        }
        Err(_) => absolute.display().to_string(),
    }
}

/// 工具输出中的路径：优先使用用户传入的相对路径（POSIX `/`），否则由绝对路径反推相对工作区路径。
pub(super) fn path_for_tool_display(
    working_dir: &Path,
    absolute: &Path,
    user_rel: Option<&str>,
) -> String {
    if let Some(s) = user_rel {
        let t = s.trim();
        if !t.is_empty() {
            return t.replace('\\', "/");
        }
    }
    path_relative_to_workspace(working_dir, absolute)
}
pub(super) fn parse_path_content(args_json: &str) -> Result<(String, String), String> {
    let v: serde_json::Value = crate::tools::parse_args_json(args_json)?;
    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(String::from)
        .ok_or_else(|| "缺少 path 参数".to_string())?;
    let content = v
        .get("content")
        .and_then(|c| c.as_str())
        .map(String::from)
        .unwrap_or_default();
    Ok((path, content))
}
