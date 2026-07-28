//! P（Plan）步相关：**一次 `complete_chat_retrying`**（[`plan_call`]）。
//!
//! 与 **`intent`** 并列，同属回合编排的子域；**禁止**在此目录外新开直达 **`llm::api::stream_chat`** 的路径。

pub(crate) mod plan_call;

pub(crate) use plan_call::{PerPlanCallModelParams, per_plan_call_model_retrying};
