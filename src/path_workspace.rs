//! 工作区路径的语义化规范化（`..` / `.`）与根边界校验（**工具与 Web 共用单一真源**）。
//!
//! ## 边界说明
//!
//! - **前缀校验**使用 [`Path::starts_with`]（按路径分量），避免 `/foo/bar` 误匹配 `/foo/bar-baz`。
//! - **`..` / 相对路径**：由 [`path_absolutize::Absolutize`] 在已 canonical 的根下解析；对**已存在路径**再 `canonicalize` 以解析符号链接真实位置。
//!
//! ## 校验与打开之间的竞态（TOCTOU）
//!
//! 在**信任工作区**的典型开发场景下，本模块在访问前对路径做 `canonicalize` 与 `starts_with` 检查，可拒绝在**检查时刻**已指向根外的符号链接等情形。
//!
//! **已知局限**：从「完成校验」到进程实际 `open` / `read_dir` 之间，同一路径字符串可能被并发替换为指向工作区外的符号链接（或父目录被重组），按路径重新打开时**不保证**仍与校验时观察到的是同一 dentry。本仓库**尚未**在全路径上采用「打开时禁止跟随」策略，因此**不要**将当前实现等同于「不可逃逸」保证；多租户或不可信工作区须与 **P0 鉴权**等一并评估。
//!
//! **更强缓解方向**（需在 `file` / Web 等调用链贯通，并处理可移植性与 API 语义）：
//!
//! - Unix：对末级或关键分量使用 **`O_NOFOLLOW`**（[`std::os::unix::fs::OpenOptionsExt::custom_flags`]）；
//! - 在已校验的**工作区根目录 fd** 上用相对分量的 **`openat`** 逐级打开，缩短窗口；
//! - Linux：评估 **`openat2`** 与 **`RESOLVE_NO_SYMLINKS`** / **`RESOLVE_IN_ROOT`** 等（内核与 MSRV 约束单独评估）。
//!
//! 用户可见说明见 **`README.md`**、**`docs/CONFIGURATION.md`**（工作区）；跟踪项见 **`docs/TODOLIST.md`** 全局安全相关条目。工具侧解析实现见 **`src/tools/file/path.rs`**。

use path_absolutize::Absolutize;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::config::AgentConfig;

/// 工作区路径解析与策略校验失败（可判别类别，供日志与调用方分支；对用户展示用 [`Display`] / [`WorkspacePathError::user_message`]）。
#[derive(Debug, Error)]
pub enum WorkspacePathError {
    /// `path` 等入参为空或仅空白。
    #[error("path 不能为空")]
    EmptyPath,
    /// 要求相对路径时收到了绝对路径。
    #[error("路径必须为相对于工作目录的相对路径，不能使用绝对路径")]
    AbsolutePathNotAllowed,
    /// 切换工作区时路径参数为空。
    #[error("路径不能为空")]
    WorkspaceSetPathEmpty,
    /// 无法取得当前工作目录（如 `getcwd` 失败）。
    #[error("无法获取当前目录: {0}")]
    CurrentDirUnavailable(#[source] std::io::Error),
    /// 路径不存在、无法 canonicalize，或不是目录等与解析/存在性相关的问题。
    #[error("工作区路径无效或不存在: {0}")]
    WorkspacePathInvalid(#[source] std::io::Error),
    /// 通用「无法解析 canonical 路径」（工具读文件、祖先校验等）。
    #[error("路径无法解析: {0}")]
    PathResolveFailed(#[source] std::io::Error),
    /// 工作区根或当前目录无法 canonicalize。
    #[error("工作目录无法解析: {0}")]
    WorkspaceResolveFailed(#[source] std::io::Error),
    /// 路径存在但不是目录。
    #[error("工作区路径必须是已存在的目录")]
    NotADirectory,
    /// 命中敏感系统目录前缀黑名单。
    #[error("工作区路径命中敏感目录黑名单，请选择业务目录")]
    SensitivePathDenied,
    /// 工作区根命中敏感前缀（与切换路径文案略异，便于区分场景）。
    #[error("工作区根路径命中敏感目录黑名单")]
    EffectiveRootSensitive,
    /// 路径落在允许根集合之外（策略拒绝）。
    #[error("工作区路径不在允许范围内（须位于以下根目录之一下: {roots_display})")]
    OutsideAllowedRoots { roots_display: String },
    /// 当前生效工作区根不在允许范围内。
    #[error("工作区根不在允许范围内（须位于以下根目录之一: {roots_display})")]
    EffectiveRootOutsideAllowed { roots_display: String },
    /// 规范化后路径越过工作区根（`..` 逃逸或 Web 子路径越界）。
    #[error("路径不能超出工作目录")]
    OutsideWorkspaceRoot,
    /// `path_absolutize` 词法规范化失败（`absolutize` / `absolutize_from` 的 IO 错误）。
    #[error("路径规范化失败: {0}")]
    NormalizationFailed(#[source] std::io::Error),
    /// 自根向上找不到任何存在祖先（极少见，如根被删）。
    #[error("路径无法解析")]
    NoExistingAncestor,
}

impl WorkspacePathError {
    /// 与历史 `String` 错误语义一致的简短分类，便于 metrics / 结构化日志（不含敏感路径全量时可只记此项）。
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            WorkspacePathError::EmptyPath => "empty_path",
            WorkspacePathError::AbsolutePathNotAllowed => "absolute_path_not_allowed",
            WorkspacePathError::WorkspaceSetPathEmpty => "workspace_set_path_empty",
            WorkspacePathError::CurrentDirUnavailable(_) => "current_dir_unavailable",
            WorkspacePathError::WorkspacePathInvalid(_) => "workspace_path_invalid",
            WorkspacePathError::PathResolveFailed(_) => "path_resolve_failed",
            WorkspacePathError::WorkspaceResolveFailed(_) => "workspace_resolve_failed",
            WorkspacePathError::NotADirectory => "not_a_directory",
            WorkspacePathError::SensitivePathDenied => "sensitive_path_denied",
            WorkspacePathError::EffectiveRootSensitive => "effective_root_sensitive",
            WorkspacePathError::OutsideAllowedRoots { .. } => "outside_allowed_roots",
            WorkspacePathError::EffectiveRootOutsideAllowed { .. } => {
                "effective_root_outside_allowed"
            }
            WorkspacePathError::OutsideWorkspaceRoot => "outside_workspace_root",
            WorkspacePathError::NormalizationFailed(_) => "path_normalize_failed",
            WorkspacePathError::NoExistingAncestor => "no_existing_ancestor",
        }
    }

    /// 是否属于「策略/权限」类（越界、敏感目录、允许根外）；用于 HTTP 403 等映射。
    #[must_use]
    pub fn is_policy_denied(&self) -> bool {
        matches!(
            self,
            WorkspacePathError::SensitivePathDenied
                | WorkspacePathError::EffectiveRootSensitive
                | WorkspacePathError::OutsideAllowedRoots { .. }
                | WorkspacePathError::EffectiveRootOutsideAllowed { .. }
                | WorkspacePathError::OutsideWorkspaceRoot
                | WorkspacePathError::AbsolutePathNotAllowed
        )
    }

    /// 面向用户/API 的说明（与实现 `Display` 一致，便于显式调用）。
    #[must_use]
    pub fn user_message(&self) -> String {
        self.to_string()
    }
}

/// 校验用于切换工作区根的 `path`（Web **`POST /workspace`** 与 REPL **`/workspace`** 共用）。
///
/// 须为已存在目录，`canonicalize` 后落在 **`workspace_allowed_roots`** 内且不得命中敏感路径黑名单。
/// 相对路径相对于**进程当前工作目录**解析（与历史 Web 行为一致）。
pub(crate) fn validate_workspace_set_path(
    cfg: &AgentConfig,
    raw: &str,
) -> Result<PathBuf, WorkspacePathError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(WorkspacePathError::WorkspaceSetPathEmpty);
    }
    let cwd = std::env::current_dir().map_err(WorkspacePathError::CurrentDirUnavailable)?;
    let p = Path::new(raw);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    };
    let canon = joined
        .canonicalize()
        .map_err(WorkspacePathError::WorkspacePathInvalid)?;
    if !canon.is_dir() {
        return Err(WorkspacePathError::NotADirectory);
    }
    if is_sensitive_workspace_path(&canon) {
        return Err(WorkspacePathError::SensitivePathDenied);
    }
    if !is_within_allowed_roots(&canon, &cfg.workspace_allowed_roots) {
        let roots_display = cfg
            .workspace_allowed_roots
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(WorkspacePathError::OutsideAllowedRoots { roots_display });
    }
    Ok(canon)
}

/// Web `POST /workspace` 与「当前会话工作区根」校验共用的敏感路径前缀（canonical 后命中即拒绝）。
const SENSITIVE_WORKSPACE_PREFIXES: &[&str] = &[
    "/proc", "/sys", "/dev", "/etc", "/boot", "/root", "/bin", "/sbin", "/usr",
];

/// 将工作目录（可为符号链接）解析为 **canonical** 绝对路径，供工具与 Web 共用。
pub(crate) fn canonical_workspace_root(base: &Path) -> Result<PathBuf, WorkspacePathError> {
    base.canonicalize()
        .map_err(WorkspacePathError::WorkspaceResolveFailed)
}

/// 规范化后的路径是否命中敏感系统目录前缀（用于拒绝把工作区设到或解析到此类路径）。
pub(crate) fn is_sensitive_workspace_path(path: &Path) -> bool {
    SENSITIVE_WORKSPACE_PREFIXES.iter().any(|prefix| {
        let p = Path::new(prefix);
        path == p || path.starts_with(p)
    })
}

/// `candidate` 与 `root` 均须已为 **canonical** 路径；要求 `candidate == root` 或 `candidate` 为 `root` 之下的子孙路径。错误文案与工具层越界一致。
pub(crate) fn ensure_canonical_within_root(
    candidate: &Path,
    root: &Path,
) -> Result<(), WorkspacePathError> {
    if candidate.starts_with(root) {
        Ok(())
    } else {
        Err(WorkspacePathError::OutsideWorkspaceRoot)
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
) -> Result<(), WorkspacePathError> {
    if is_sensitive_workspace_path(base_canonical) {
        return Err(WorkspacePathError::EffectiveRootSensitive);
    }
    if !is_within_allowed_roots(base_canonical, &cfg.workspace_allowed_roots) {
        let roots_display = cfg
            .workspace_allowed_roots
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(WorkspacePathError::EffectiveRootOutsideAllowed { roots_display });
    }
    Ok(())
}

/// 自 `target` 向上找到最近存在路径并 canonicalize，须落在 `root_canonical` 下（写入路径防 symlink 逃逸）。
pub(crate) fn ensure_existing_ancestor_within_root(
    root_canonical: &Path,
    target: &Path,
) -> Result<(), WorkspacePathError> {
    let mut ancestor = target;
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or(WorkspacePathError::NoExistingAncestor)?;
    }
    let ancestor_canonical = ancestor
        .canonicalize()
        .map_err(WorkspacePathError::PathResolveFailed)?;
    ensure_canonical_within_root(&ancestor_canonical, root_canonical)
}

/// `sub` 必须为相对路径；在已 canonical 的 `workspace_root` 下解析并去掉 `.` / `..`，且不得越出根。
pub(crate) fn absolutize_relative_under_root(
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
    let normalized = Path::new(sub)
        .absolutize_from(workspace_root)
        .map_err(WorkspacePathError::NormalizationFailed)?;
    if !normalized.starts_with(workspace_root) {
        return Err(WorkspacePathError::OutsideWorkspaceRoot);
    }
    Ok(normalized.into_owned())
}

/// Web 工作区写入等：`sub` 可为绝对或相对路径；规范化后须落在 `base_canonical` 之下。
pub(crate) fn absolutize_workspace_subpath(
    base_canonical: &Path,
    sub: &str,
) -> Result<PathBuf, WorkspacePathError> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err(WorkspacePathError::EmptyPath);
    }
    let normalized = if Path::new(sub).is_absolute() {
        Path::new(sub)
            .absolutize()
            .map_err(WorkspacePathError::NormalizationFailed)?
    } else {
        Path::new(sub)
            .absolutize_from(base_canonical)
            .map_err(WorkspacePathError::NormalizationFailed)?
    };
    if !normalized.starts_with(base_canonical) {
        return Err(WorkspacePathError::OutsideWorkspaceRoot);
    }
    Ok(normalized.into_owned())
}

/// Web：在已 canonical 的工作区根下解析只读路径；`sub` 缺省或空则返回根本身。
pub(crate) fn resolve_web_workspace_read_path(
    base_canonical: &Path,
    sub: Option<&str>,
) -> Result<PathBuf, WorkspacePathError> {
    let sub = match sub {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return Ok(base_canonical.to_path_buf()),
    };
    let normalized = absolutize_workspace_subpath(base_canonical, sub)?;
    let canonical = normalized
        .canonicalize()
        .map_err(WorkspacePathError::PathResolveFailed)?;
    ensure_canonical_within_root(&canonical, base_canonical)?;
    Ok(canonical)
}

/// Web：解析写入路径（目标可不存在）；防 symlink 逃逸。
pub(crate) fn resolve_web_workspace_write_path(
    base_canonical: &Path,
    sub: &str,
) -> Result<PathBuf, WorkspacePathError> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err(WorkspacePathError::EmptyPath);
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

    #[test]
    fn validate_workspace_set_path_rejects_empty() {
        let cfg = crate::config::load_config(None).expect("embedded default config");
        let e = validate_workspace_set_path(&cfg, "  ").expect_err("empty");
        assert_eq!(e.kind(), "workspace_set_path_empty");
        let msg = e.user_message();
        assert!(msg.contains("空"), "{msg}");
    }

    #[test]
    fn outside_workspace_root_kind() {
        let e = WorkspacePathError::OutsideWorkspaceRoot;
        assert_eq!(e.kind(), "outside_workspace_root");
    }
}
