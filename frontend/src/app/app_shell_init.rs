//! App 壳初始化：将 `wire_*` 调用与 `AppShellCtx` 组装委托给 [`super::app_shell_bootstrap`]。
//!
//! # 接线顺序
//!
//! 单一事实来源为 [`bootstrap_app_shell`](super::app_shell_bootstrap::bootstrap_app_shell) 内的
//! [`run_shell_wiring_in_order`](super::app_shell_wire_phases::run_shell_wiring_in_order)；
//! 阶段表见 [`super::app_shell_wire_phases`]。

pub use super::app_shell_bootstrap::bootstrap_app_shell;

/// 历史入口名，等价于 [`bootstrap_app_shell`].
#[inline]
pub fn init_app_shell() -> super::app_shell_ctx::AppShellCtx {
    bootstrap_app_shell()
}
