//! Operator Agent：执行子目标的 ReAct 循环（拆分为子模块便于维护）。

mod agent_impl;
mod compile;
mod inject;
mod prompt;
mod react_loop;
mod state;
mod text;
mod types;

#[cfg(test)]
mod tests;

pub use types::{CompileErrorInfo, CompileErrorType, OperatorAgent, OperatorConfig, OperatorError};
