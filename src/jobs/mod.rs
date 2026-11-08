//! 后台任务桶（S4-P1c 建，S4-P2-3 起成员已迁入，见 #1300 §1.4）。
//!
//! 成员只有 1 子项（`tool_jobs`，8 文件 / 2850 行）。单成员桶看着多余，但它的引用面与
//! `tool_host` 完全正交，故独立一期搬迁。
//!
//! 上层一律写 `crate::jobs::tool_jobs::…`：`tool_jobs` 没有根上短名消费者，
//! `lib_server.rs` 不导出它，所以 **没有** `crate::tool_jobs::…` 这种写法。

pub mod tool_jobs;
