//! [`crate::cm_types::permission_audit`] 的便捷包装：接受执行面手持的
//! [`crate::cm_agent::permission::PolicyEngine`] 端口（S3-b）。
//!
//! 审计的**实现**在 `cm_types`（叶子层），因为 `cm_tools` 的 `PathAccess` 也要记路径轴；
//! 本模块只做「从端口取 `level()` / `approval()`」这一步，让执行面的调用点不必各自展开。
//! 传 `&PermissionCaps` 的既有调用点靠自动 coerce 继续可用。
//! 语义与判定规则见 `cm_types::permission_audit` 的模块文档。

pub use crate::cm_types::permission_audit::{
    PermissionAuditEvent, PermissionAxis, PermissionDecision, emit, record_approved, should_emit,
};
use crate::cm_agent::permission::PolicyEngine;

/// 等级**直接放行**（L4 的核心留痕）。L0–L3 下是空操作。
///
/// 收 `&dyn PolicyEngine` 而不是 `&PermissionCaps`（S3-b）：执行面手持端口后无需再降级
/// 回具体类型；`&PermissionCaps` 在调用点自动 coerce，既有调用点不用改。
pub fn record_allowed(
    engine: &dyn PolicyEngine,
    axis: PermissionAxis,
    tool: Option<&str>,
    detail: Option<&str>,
) {
    crate::cm_types::permission_audit::record_allowed(
        engine.level(),
        engine.approval(),
        axis,
        tool,
        detail,
    );
}

/// 等级**直接拒绝**（L0–L2 的硬边界，L4 的 external 轴也可能拒）。
pub fn record_denied(
    engine: &dyn PolicyEngine,
    axis: PermissionAxis,
    tool: Option<&str>,
    detail: Option<&str>,
) {
    crate::cm_types::permission_audit::record_denied(
        engine.level(),
        engine.approval(),
        axis,
        tool,
        detail,
    );
}
