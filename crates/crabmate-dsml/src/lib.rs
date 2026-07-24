//! DeepSeek DSML：正文工具调用解析、物化与展示剥离。

mod adapter;
mod normalizer;
mod parser;
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

#[cfg(test)]
mod tests_materialize_tail;
