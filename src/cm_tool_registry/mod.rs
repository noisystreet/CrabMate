//! 工具**注册面**的中立部分（#1510 P3-3 第一批）。
//!
//! 装的是「参数类型（`param_types`）+ JSON Schema 生成（`json_schema`）」这两块
//! **零 `crate::` 依赖**的纯数据/纯函数：它们既被注册表（`specs_registry` 的
//! `builtin_tool!` 从 Args 派生 schema）用，也被各工具实现用。
//!
//! 之所以要出工具层：`cm_tools` 被上层广泛依赖，任何「谁都要用」的东西留在里面，
//! 迟早逼出反向依赖。这里两块自身零依赖，搬出来成本最低、收益明确。
//!
//! **未搬**：`tools/tool_specs_registry/`。它经 `use super::*` 用到 `ToolSpec`
//! （定义在 `tools/runners.rs`，与 `runner_*` 实现混居）并依赖 `tool_summary_args`
//! （1983 行），需先拆 `runners.rs` 才能干净下沉 —— 属后续批次。

pub mod json_schema;
pub mod param_types;
