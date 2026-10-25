//! 工作区路径解析与校验（`file` 工具子模块）。
//!
//! 边界语义与 [`crate::cm_tools::workspace::path`] 一致。读路径优先经 [`resolve_for_read_open`]：Linux 上在已打开的工作区根 fd 上使用 **`openat2` + `RESOLVE_IN_ROOT`** 打开，将解析约束在根内并避免「校验后再次按路径 `open`」的窗口。其余局限见 [`crate::cm_tools::workspace::path`] 与 [`crate::cm_tools::workspace::fs`]。
#![allow(clippy::manual_string_new)]

use std::path::{Path, PathBuf};

use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::workspace::fs::OpenedWorkspaceFile;
use crate::cm_tools::workspace::path::{
    WorkspacePathError, absolutize_relative_under_root, ensure_canonical_within_root,
    ensure_existing_ancestor_within_root,
};

pub use crate::cm_tools::workspace::path::canonical_workspace_root;

/// 将 [`WorkspacePathError`] 格式化为工具返回给模型的前缀文案（与历史 `错误：…` 一致）。
#[must_use]
pub fn tool_user_error_from_workspace_path(e: WorkspacePathError) -> String {
    e.prefixed_user_message()
}

/// 只读工具在工作区路径解析失败时要用的两个稳定错误码（由调用方给出，保持各工具的码名前缀）。
pub(super) struct WorkspacePathCodes {
    /// 路径落在工作区根之外，或工作区自身不可解析（`workspace` 类别）。
    pub outside: &'static str,
    /// 目标不存在、打开失败等 IO 侧原因（`external` 类别）。
    pub io: &'static str,
}

/// [`WorkspacePathError`] 的对外分类（决定 `error_code` 与 `failure_category`）。
enum WorkspacePathErrorClass {
    /// 路径越界，或工作区自身不可解析。
    Workspace,
    /// 目标不存在、打开失败等 IO 侧原因。
    Io,
    /// 入参问题（空路径、绝对路径、规范化失败）。
    InvalidArgs,
}

fn classify_workspace_path_error(e: &WorkspacePathError) -> WorkspacePathErrorClass {
    match e {
        WorkspacePathError::OutsideWorkspaceRoot
        | WorkspacePathError::WorkspaceResolveFailed(_)
        | WorkspacePathError::NoExistingAncestor => WorkspacePathErrorClass::Workspace,
        WorkspacePathError::PathResolveFailed(_) => WorkspacePathErrorClass::Io,
        _ => WorkspacePathErrorClass::InvalidArgs,
    }
}

fn build_workspace_path_error(
    class: WorkspacePathErrorClass,
    codes: WorkspacePathCodes,
    message: String,
) -> ToolError {
    match class {
        WorkspacePathErrorClass::Workspace => ToolError::workspace(codes.outside, message),
        WorkspacePathErrorClass::Io => ToolError::external_code(codes.io, message),
        WorkspacePathErrorClass::InvalidArgs => ToolError::invalid_args(message),
    }
}

/// 把 [`WorkspacePathError`] 分类为 [`ToolError`]，正文沿用 `错误：…` 文案。
///
/// 为什么要分：`resolve_for_read` 对「越界」「文件不存在」「入参非法」返回同一个枚举，
/// 若统一映射成 `*_workspace_resolve_failed`，模型会把最常见的「文件不存在」误读成工作区配置问题。
pub(super) fn workspace_path_tool_error(
    e: WorkspacePathError,
    codes: WorkspacePathCodes,
) -> ToolError {
    let class = classify_workspace_path_error(&e);
    build_workspace_path_error(class, codes, tool_user_error_from_workspace_path(e))
}

/// 同 [`workspace_path_tool_error`]，但由调用方给出正文（如 `错误：无法解析目录路径：…`），保持历史文案不变。
pub(super) fn workspace_path_tool_error_with_message(
    e: &WorkspacePathError,
    codes: WorkspacePathCodes,
    message: String,
) -> ToolError {
    build_workspace_path_error(classify_workspace_path_error(e), codes, message)
}

/// 若 `sub` 为**已落在工作区根下**的绝对路径，则转为工作区相对路径；否则原样返回（并 trim）。  
/// 供 `read`/`write` 共用，减少模型误传 `/home/.../proj/foo` 时与「仅相对路径」规则冲突。
pub fn normalize_subpath_for_workspace(
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
pub fn resolve_for_read(base: &Path, sub: &str) -> Result<PathBuf, WorkspacePathError> {
    Ok(resolve_for_read_open(base, sub)?.resolved_path)
}

/// 与 [`resolve_for_read`] 相同策略校验，但用 **`openat2` / `RESOLVE_IN_ROOT`（Linux）** 或单次 `File::open` 打开，返回的 `metadata` 与 `file` 对应同一打开，缓解校验后二次按路径 `open` 的 TOCTOU。
pub fn resolve_for_read_open(
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
    crate::cm_tools::workspace::fs::open_existing_file_under_root(&base_canonical, &canonical).map_err(|e| {
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

/// 词法归一化：折叠 `.` 与 `..`，不要求路径存在，也不做「是否越界」判断。
///
/// 只用于 `PathScope::Any`（L4）的写路径：目标可能尚不存在，`canonicalize` 用不了，
/// 但 `..` 必须按路径语义折叠，否则 `a/../b` 与 `b` 会在审计里显示成两条路径。
fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                // 已在根（`/`）时 `pop` 返回 false，保持根不变。
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 把「绝对 / 相对」统一成待解析的绝对路径（相对路径按会话根拼接）。
fn unrestricted_target(base: &Path, sub: &str) -> PathBuf {
    let p = Path::new(sub);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

/// `PathScope::Any` 的读解析：目标必须存在，**不**校验是否落在工作区根内。
///
/// 安全边界：L4 显式放宽工作区约束（设计 §4 / §9），因此这里不再有 `openat2`
/// `RESOLVE_IN_ROOT` 那层保护——路径由模型在 L4 下给出，属运维有意授权。
pub fn resolve_unrestricted_for_read(base: &Path, sub: &str) -> Result<PathBuf, WorkspacePathError> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err(WorkspacePathError::EmptyPath);
    }
    unrestricted_target(base, sub)
        .canonicalize()
        .map_err(WorkspacePathError::PathResolveFailed)
}

/// [`resolve_unrestricted_for_read`] 的打开版本（无 `RESOLVE_IN_ROOT`，见其安全说明）。
pub fn resolve_unrestricted_for_read_open(
    base: &Path,
    sub: &str,
) -> Result<OpenedWorkspaceFile, WorkspacePathError> {
    let target = resolve_unrestricted_for_read(base, sub)?;
    crate::cm_tools::workspace::fs::open_existing_file_unrestricted(&target).map_err(|e| {
        WorkspacePathError::PathResolveFailed(std::io::Error::new(
            e.kind(),
            format!("open unrestricted: {e}"),
        ))
    })
}

/// `PathScope::Any` 的写解析：目标可不存在，**不**校验是否落在工作区根内。
pub fn resolve_unrestricted_for_write(base: &Path, sub: &str) -> Result<PathBuf, WorkspacePathError> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err(WorkspacePathError::EmptyPath);
    }
    Ok(lexical_normalize(&unrestricted_target(base, sub)))
}

/// 审计正文只用**末两段**路径。
///
/// 完整绝对路径会把宿主机目录结构写进日志（可能被采集到远端），而「访问了哪个文件」
/// 用末两段已足够定位；需要全路径时以同一时间窗口的 `tool_call` 参数为准。
pub(crate) fn audit_path_detail(sub: &str) -> String {
    let mut tail: Vec<&str> = sub.rsplit('/').filter(|s| !s.is_empty()).take(2).collect();
    tail.reverse();
    if tail.is_empty() {
        "-".to_string()
    } else {
        tail.join("/")
    }
}

/// 工作区路径访问器：把「根 + 访问范围」收成一个载体。
///
/// R1（设计 §17.1）收口调用形态；S3 起持有 [`PathScope`]：`SessionRoot`（默认，L0–L3）
/// 仍走 [`resolve_for_read`] / [`resolve_for_write`] 的越界校验，`Any`（L4）走
/// [`resolve_unrestricted_for_read`] / [`resolve_unrestricted_for_write`]。
/// 调用点不必知道自己在哪个档位，只需把 `ctx.path_scope` 传进来。
pub struct PathAccess<'a> {
    root: &'a Path,
    scope: crate::cm_types::PathScope,
}

impl<'a> PathAccess<'a> {
    /// 默认 `SessionRoot`（与 S3 之前逐字等价）。
    #[must_use]
    pub fn new(root: &'a Path) -> Self {
        Self::with_scope(root, crate::cm_types::PathScope::SessionRoot)
    }

    /// 带路径范围构造（主 Agent 的 `dispatch` 与工作流节点用这一条）。
    #[must_use]
    pub fn with_scope(root: &'a Path, scope: crate::cm_types::PathScope) -> Self {
        Self { root, scope }
    }

    /// 工作区根（供 `canonical_workspace_root` / `path_for_tool_display` 复用）。
    #[must_use]
    pub fn root(&self) -> &'a Path {
        self.root
    }

    /// 当前路径范围。
    #[must_use]
    pub fn scope(&self) -> crate::cm_types::PathScope {
        self.scope
    }

    /// L4 的路径放行留痕。
    ///
    /// 等级直接回填 `Unrestricted`：`PathScope::Any` **只**由 `project_caps(Unrestricted)` 产生
    /// （唯一投影点），五级投影里只有 L4 的 `path_scope` 是 `any`——
    /// 由 `cm_types::permission_audit::tests::path_scope_any_exists_only_at_unrestricted` 钉住。
    /// 本层拿不到 `PermissionCaps`（依赖方向 `cm_tools` → `cm_agent` 不允许反向）。
    fn audit_unrestricted_path(&self, sub: &str) {
        crate::cm_types::permission_audit::record_allowed(
            crate::cm_types::PermissionLevel::Unrestricted,
            crate::cm_types::ApprovalPolicy::AuditOnly,
            crate::cm_types::permission_audit::PermissionAxis::Path,
            None,
            Some(&audit_path_detail(sub)),
        );
    }

    /// 见 [`resolve_for_read`]。
    pub fn for_read(&self, sub: &str) -> Result<PathBuf, WorkspacePathError> {
        if self.scope.is_unrestricted() {
            self.audit_unrestricted_path(sub);
            return resolve_unrestricted_for_read(self.root, sub);
        }
        resolve_for_read(self.root, sub)
    }

    /// 见 [`resolve_for_read_open`]（`openat2` / `RESOLVE_IN_ROOT`）。
    pub fn for_read_open(&self, sub: &str) -> Result<OpenedWorkspaceFile, WorkspacePathError> {
        if self.scope.is_unrestricted() {
            self.audit_unrestricted_path(sub);
            return resolve_unrestricted_for_read_open(self.root, sub);
        }
        resolve_for_read_open(self.root, sub)
    }

    /// 见 [`resolve_for_write`]。
    pub fn for_write(&self, sub: &str) -> Result<PathBuf, WorkspacePathError> {
        if self.scope.is_unrestricted() {
            self.audit_unrestricted_path(sub);
            return resolve_unrestricted_for_write(self.root, sub);
        }
        resolve_for_write(self.root, sub)
    }
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
    let v: serde_json::Value = crate::cm_tools::tools::parse_args_json(args_json)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_types::PathScope;

    /// S3 路径轴：`Any`（L4）可读到工作区根之外，`SessionRoot`（L0–L3）仍拒。
    #[test]
    fn unrestricted_scope_reads_outside_workspace_root() {
        let base = std::env::temp_dir().join(format!("crabmate_s3_path_{}", std::process::id()));
        let root = base.join("root");
        let outside = base.join("outside.txt");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::write(&outside, b"x").expect("write probe");
        let outside_str = outside.to_str().expect("utf8 temp path");

        let any = PathAccess::with_scope(&root, PathScope::Any);
        assert!(any.for_read(outside_str).is_ok(), "L4 应能读工作区外");
        assert!(any.for_write(outside_str).is_ok(), "L4 应能写工作区外");

        let scoped = PathAccess::new(&root);
        assert!(
            scoped.for_read(outside_str).is_err(),
            "SessionRoot 必须拒绝工作区外绝对路径"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    /// `..` 在 L4 写路径里按词法折叠（目标可不存在），不残留 `..` 分量。
    #[test]
    fn unrestricted_write_normalizes_parent_dir() {
        let path = resolve_unrestricted_for_write(std::path::Path::new("/tmp/root"), "a/../b.txt")
            .expect("resolve");
        assert_eq!(path, std::path::PathBuf::from("/tmp/root/b.txt"));
    }

    /// 接线回归（High）：L4 的路径放行必须留痕——这是路径面**唯一**的记录点，
    /// `read_file` / `write` / `delete_*` 等全部经 `PathAccess` 走这里。
    #[test]
    fn unrestricted_scope_emits_path_audit() {
        use crate::cm_types::permission_audit::test_support::captured_with;

        let any = PathAccess::with_scope(std::path::Path::new("/tmp/root"), PathScope::Any);
        let captured = captured_with(|| {
            // 目标不存在 → `Err`，但审计在解析前就记了（放行意图本身就是审计对象）。
            let _ = any.for_read("/etc/shadow");
        });
        assert!(captured.contains("permission_axis=path"), "{captured}");
        assert!(
            captured.contains("permission_level=unrestricted"),
            "{captured}"
        );
        // 只留末两段：完整绝对路径不进日志。
        assert!(captured.contains("detail=etc/shadow"), "{captured}");
        assert!(!captured.contains("/tmp/root"), "{captured}");

        // L0–L3（`SessionRoot`）是常态，不记。
        let scoped = PathAccess::new(std::path::Path::new("/tmp/root"));
        let quiet = captured_with(|| {
            let _ = scoped.for_read("a.txt");
        });
        assert!(quiet.is_empty(), "{quiet}");
    }
}
