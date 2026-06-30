//! Operator Agent：执行子目标的 ReAct 循环（拆分为子模块便于维护）。

mod agent_impl;
mod compile;
mod compile_error_match;
mod execution_guides;
mod inject;
mod operator_tool_analysis;
mod prompt;
mod react_loop;
mod react_loop_helpers;
mod state;
mod text;
mod types;

#[cfg(test)]
mod tests;

pub use types::{
    CompileErrorInfo, CompileErrorType, OperatorAgent, OperatorConfig, OperatorError,
    OperatorPolicy, OperatorRuntimeHandles,
};
