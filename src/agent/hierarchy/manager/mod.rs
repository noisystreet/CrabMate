//! Manager Agent：任务分解与协调（子模块拆分）。

mod agent_core;
#[cfg(test)]
mod agent_tests;
mod manager_prompts;
mod manager_tail;
mod output_parse;
mod reflect;
mod session_compile;
mod types;

pub use manager_tail::{FailureDecision, ManagerOutput, handle_failure};
pub use types::{ManagerAgent, ManagerConfig, ManagerDecision, ManagerError};
