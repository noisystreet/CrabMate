//! 工作区路径的语义化规范化（`..` / `.`）与根边界校验（**工具与 Web 共用单一真源**）。
//!
//! ## 边界说明
//!
//! - **前缀校验**使用 [`Path::starts_with`]（按路径分量），避免 `/foo/bar` 误匹配 `/foo/bar-baz`。
//! - **`..` / 相对路径**：由 [`path_absolutize::Absolutize`] 在已 canonical 的根下解析；对**已存在路径**再 `canonicalize` 以解析符号链接真实位置。
//! - **竞态**：`canonicalize` 与打开文件之间路径可能被替换为指向根外的 symlink；完全堵住需 `O_NOFOLLOW` 等，与 **TODOLIST** 已知项一致，此处为尽力校验。

use path_absolutize::Absolutize;
use std::path::{Path, PathBuf};

use crate::config::AgentConfig;

/// Web `POST /workspace` 与「当前会话工作区根」校验共用的敏感路径前缀（canonical 后命中即拒绝）。
const SENSITIVE_WORKSPACE_PREFIXES: &[&str] = &[
    "/proc", "/sys", "/dev", "/etc", "/boot", "/root", "/bin", "/sbin", "/usr",
];

/// 将工作目录（可为符号链接）解析为 **canonical** 绝对路径，供工具与 Web 共用。
pub(crate) fn canonical_workspace_root(base: &Path) -> Result<PathBuf, String> {
    base.canonicalize()
        .map_err(|e| format!("工作目录无法解析: {}", e))
}

/// 规范化后的路径是否命中敏感系统目录前缀（用于拒绝把工作区设到或解析到此类路径）。
pub(crate) fn is_sensitive_workspace_path(path: &Path) -> bool {
    SENSITIVE_WORKSPACE_PREFIXES.iter().any(|prefix| {
        let p = Path::new(prefix);
        path == p || path.starts_with(p)
    })
}

/// `candidate` 与 `root` 均须已为 **canonical** 路径；要求 `candidate == root` 或 `candidate` 为 `root` 之下的子孙路径。错误文案与工具层越界一致。
pub(crate) fn ensure_canonical_within_root(candidate: &Path, root: &Path) -> Result<(), String> {
    if candidate.starts_with(root) {
        Ok(())
    } else {
        Err("路径不能超出工作目录".to_string())
    }
}

/// `candidate`（已 canonical）是否落在任一 **canonical** 允许根之下（配置 `workspace_allowed_roots`）。
pub(crate) fn is_within_allowed_roots(candidate: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| candidate.starts_with(r))
}

/// 校验「当前生效的工作区根」仍合法：非敏感目录且在 `workspace_allowed_roots` 内。
pub(crate) fn validate_effective_workspace_base(
    cfg: &AgentConfig,
    base_canonical: &Path,
) -> Result<(), String> {
    if is_sensitive_workspace_path(base_canonical) {
        return Err("工作区根路径命中敏感目录黑名单".to_string());
    }
    if !is_within_allowed_roots(base_canonical, &cfg.workspace_allowed_roots) {
        let roots = cfg
            .workspace_allowed_roots
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "工作区根不在允许范围内（须位于以下根目录之一: {}）",
            roots
        ));
    }
    Ok(())
}

fn ensure_existing_ancestor_within_root(
    root_canonical: &Path,
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
    ensure_canonical_within_root(&ancestor_canonical, root_canonical)
}

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
        return Err("路径不能超出工作目录".to_string());
    }
    Ok(normalized.into_owned())
}

/// Web：在已 canonical 的工作区根下解析只读路径；`sub` 缺省或空则返回根本身。
pub(crate) fn resolve_web_workspace_read_path(
    base_canonical: &Path,
    sub: Option<&str>,
) -> Result<PathBuf, String> {
    let sub = match sub {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return Ok(base_canonical.to_path_buf()),
    };
    let normalized = absolutize_workspace_subpath(base_canonical, sub)?;
    let canonical = normalized
        .canonicalize()
        .map_err(|e| format!("路径无法解析: {}", e))?;
    ensure_canonical_within_root(&canonical, base_canonical)?;
    Ok(canonical)
}

/// Web：解析写入路径（目标可不存在）；防 symlink 逃逸。
pub(crate) fn resolve_web_workspace_write_path(
    base_canonical: &Path,
    sub: &str,
) -> Result<PathBuf, String> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err("path 不能为空".to_string());
    }
    let normalized = absolutize_workspace_subpath(base_canonical, sub)?;
    ensure_existing_ancestor_within_root(base_canonical, &normalized)?;
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn sensitive_workspace_path_matches_prefixes() {
        assert!(is_sensitive_workspace_path(Path::new("/proc")));
        assert!(is_sensitive_workspace_path(Path::new("/proc/1234")));
        assert!(is_sensitive_workspace_path(Path::new("/etc/nginx")));
        assert!(!is_sensitive_workspace_path(Path::new("/workspace")));
        assert!(!is_sensitive_workspace_path(Path::new("/tmp/project")));
    }

    #[test]
    fn starts_with_root_is_component_wise() {
        let root = Path::new("/tmp/workspace");
        let inside = Path::new("/tmp/workspace/project");
        let sibling = Path::new("/tmp/workspace2");
        assert!(ensure_canonical_within_root(inside, root).is_ok());
        assert!(ensure_canonical_within_root(sibling, root).is_err());
    }
}
