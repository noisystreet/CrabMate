//! 终答规划重写与侧向语义检查相关的**纯逻辑**（历史扫描、重写 user 文案、用尽原因分类）。
//! 不持有 `PerCoordinator` 状态；由 [`crate::agent::per_coord::PerCoordinator`] 调用本模块函数并维护计数器。
//!
//! **工作流反思**的状态机仍在 [`crate::agent::workflow_reflection_controller`]，经 `per_coord::prepare_workflow_execute` 驱动。

pub(crate) mod plan_rewrite;
