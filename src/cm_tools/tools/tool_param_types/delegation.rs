//! `delegate_task` 入参（#839 P1）。
//!
//! 只暴露**任务描述**：深度、工具集、预算、回传上限全部由 agent 层按父回合状态决定，
//! 不能由模型通过参数放大（那是提权口子）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `delegate_task` 入参。
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DelegateTaskArgs {
    /// 子任务目标（唯一入口）。父回合的历史、工具日志、system 全文**都不会**传给子任务。
    pub prompt: String,
}
