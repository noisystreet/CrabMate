//! 工作区路径解析与校验（`file` 工具子模块）。
#![allow(clippy::manual_string_new)]

use std::path::{Path, PathBuf};

use crate::path_workspace::{absolutize_relative_under_root, ensure_canonical_within_root};

pub(crate) use crate::path_workspace::canonical_workspace_root;

// 对“目标路径或其最近存在祖先”做 canonical 边界校验，防止借助工作区内 symlink 逃逸。
fn ensure_existing_ancestor_within_workspace(
    base_canonical: &Path,
    target: &Path,
) -> Result<(), String> {
    let mut ancestor = target;
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| "路径无法解析".to_string())?;
    }
    let ancestor_canonical = ancestor
        .canonicalize()
        .map_err(|e| format!("路径无法解析: {}", e))?;
    ensure_canonical_within_root(&ancestor_canonical, base_canonical)
}

/// 解析用于读取或修改的路径（目标必须存在；path 必须为相对工作目录的相对路径）
pub(crate) fn resolve_for_read(base: &Path, sub: &str) -> Result<PathBuf, String> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err("path 不能为空".to_string());
    }
    if Path::new(sub).is_absolute() {
        return Err("路径必须为相对于工作目录的相对路径，不能使用绝对路径".to_string());
    }
    let base_canonical = canonical_workspace_root(base)?;
    let joined = base_canonical.join(sub);
    let canonical = joined
        .canonicalize()
        .map_err(|e| format!("路径无法解析: {}", e))?;
    ensure_canonical_within_root(&canonical, &base_canonical)?;
    Ok(canonical)
}

/// 解析用于写入的路径（目标可不存在；path 必须为相对工作目录的相对路径，且不能通过 .. 超出工作目录）
pub(super) fn resolve_for_write(base: &Path, sub: &str) -> Result<PathBuf, String> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err("path 不能为空".to_string());
    }
    if Path::new(sub).is_absolute() {
        return Err("路径必须为相对于工作目录的相对路径，不能使用绝对路径".to_string());
    }
    let base_canonical = canonical_workspace_root(base)?;
    let normalized = absolutize_relative_under_root(&base_canonical, sub)?;
    ensure_existing_ancestor_within_workspace(&base_canonical, &normalized)?;
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
    let v: serde_json::Value =
        serde_json::from_str(args_json).map_err(|e| format!("参数 JSON 无效: {}", e))?;
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
