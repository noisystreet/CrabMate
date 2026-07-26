//! P（Plan）步相关：**一次 `complete_chat_retrying`**、Web 规划轮 SSE 门控、基于 **`RunLoopParams`** 的薄封装。
//!
//! 与 **`intent`** 并列，同属回合编排的子域；**禁止**在此目录外新开直达 **`llm::api::stream_chat`** 的路径。

pub(crate) mod agent_llm_call;
pub(crate) mod plan_call;
pub(crate) mod planner_sse_gate;

pub(crate) use agent_llm_call::AgentLlmCall;
pub(crate) use plan_call::{PerPlanCallModelParams, per_plan_call_model_retrying};
pub(crate) use planner_sse_gate::PlannerSseGate;
