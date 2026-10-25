//! [`crate::cm_types::permission_audit`] 的便捷包装：接受本回合的
//! [`crate::cm_agent::permission::PermissionCaps`]。
//!
//! 审计的**实现**在 `cm_types`（叶子层），因为 `cm_tools` 的 `PathAccess` 也要记路径轴；
//! 本模块只做「从 `caps` 取 `level` / `approval`」这一步，让执行面的调用点不必各自展开。
//! 语义与判定规则见 `cm_types::permission_audit` 的模块文档。

pub use crate::cm_types::permission_audit::{
    PermissionAuditEvent, PermissionAxis, PermissionDecision, emit, record_approved, should_emit,
};
use crate::cm_agent::permission::PermissionCaps;

/// 等级**直接放行**（L4 的核心留痕）。L0–L3 下是空操作。
pub fn record_allowed(
    caps: &PermissionCaps,
    axis: PermissionAxis,
    tool: Option<&str>,
    detail: Option<&str>,
) {
    crate::cm_types::permission_audit::record_allowed(
        caps.level,
        caps.approval,
        axis,
        tool,
        detail,
    );
}

/// 等级**直接拒绝**（L0–L2 的硬边界，L4 的 external 轴也可能拒）。
pub fn record_denied(
    caps: &PermissionCaps,
    axis: PermissionAxis,
    tool: Option<&str>,
    detail: Option<&str>,
) {
    crate::cm_types::permission_audit::record_denied(caps.level, caps.approval, axis, tool, detail);
}
