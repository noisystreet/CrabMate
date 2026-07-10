//! 记忆相关工具宿主（由 `crabmate-internal` 注入，避免 `crabmate-tools` → `crabmate-memory` 环）。

use std::path::Path;

use crabmate_config::AgentConfig;

/// `codebase_semantic_search` 执行面。
pub trait CodebaseSemanticToolHost: Send + Sync {
    fn run_search(&self, args_json: &str, working_dir: &Path, max_output_len: usize) -> String;
}

/// `long_term_*` / `summarize_experience` 执行面。
pub trait LongTermMemoryToolHost: Send + Sync {
    fn dispatch(&self, tool_name: &str, args_json: &str, cfg: &AgentConfig) -> String;
}
