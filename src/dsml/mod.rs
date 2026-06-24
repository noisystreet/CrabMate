//! DeepSeek DSML：正文工具调用解析、物化与展示剥离。
//!
//! 执行轮经 [`DsmlToolCallAdapter`] 物化；分阶段无工具轮经 [`staged_policy`] 仅检测违规。

mod adapter;
mod normalizer;
mod parser;
mod staged_policy;
mod stream;
mod strip;
mod strip_scan;
mod types;

#[allow(unused_imports)]
pub use adapter::DsmlToolCallAdapter;
pub use adapter::materialize_deepseek_dsml_tool_calls_in_message;
#[allow(unused_imports)]
pub use parser::{DsmlParseOutcome, ParsedDsmlInvoke, parse_combined_assistant_text};
pub use stream::StreamingDsmlContentFilter;
pub use strip::strip_deepseek_dsml_for_display;
#[allow(unused_imports)]
pub use types::{DsmlMaterializePolicy, StagedDsmlHandling, StagedDsmlScanResult};

pub(crate) use staged_policy::{
    staged_first_planner_tool_call_total_after_materialize, staged_no_tools_materialized_count,
    staged_no_tools_scan, strip_staged_planner_message_tool_calls,
};

#[cfg(test)]
mod tests_materialize_tail;
