//! 后台任务桶（S4-P1c 门面，见 #1300 §1.4）。
//!
//! **本文件当前只做转发**：成员仍住在 `cm_internal`，P2 才搬入。
//!
//! 成员只有 1 子项（`tool_jobs`，8 文件 / 2850 行）。单成员桶看着多余，但它的引用面
//! （27 处，几乎全是 `crate::cm_internal::tool_jobs::…` 长路径）与 `tool_host` 完全正交，
//! P2 要独立搬一期，所以先给它一个落点，避免 P2 时再新造模块。

// 上层目前一律写 `crate::cm_internal::tool_jobs::…`（27 处长路径），**没有短名消费者**，
// 所以 `lib_server.rs` 不导它，这条转发暂时也没人用。P3 把长路径改指本桶后即可删掉它。
#[allow(unused_imports)]
pub(crate) use crate::cm_internal::tool_jobs;
