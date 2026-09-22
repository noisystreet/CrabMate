//! 记忆相关工具宿主（由 `crabmate-internal` 注入，避免 `crabmate-tools` → `crabmate-memory` 环）。

use std::path::Path;

use crate::cm_config::AgentConfig;

/// `codebase_semantic_search` 执行面。
pub trait CodebaseSemanticToolHost: Send + Sync {
    fn run_search(&self, args_json: &str, working_dir: &Path, max_output_len: usize) -> String;
}

/// `long_term_*` / `summarize_experience` 执行面。
pub trait LongTermMemoryToolHost: Send + Sync {
    fn dispatch(&self, tool_name: &str, args_json: &str, cfg: &AgentConfig) -> String;
}

/// `background_job_status` / `background_job_list` 执行面（只读查询后台任务注册表）。
///
/// 与 ADR `docs/design/background_tool_jobs.md` 的「不新增发起执行型工具组」约束不冲突：
/// 本 trait 只读进程内 `ToolJobRegistry`，不发起执行、不重复实现审批/白名单。
pub trait ToolJobsToolHost: Send + Sync {
    /// 按 `tool_job_id` 查状态与（终态）输出。
    fn status(&self, id: &str, max_output_len: usize) -> String;
    /// 列出任务（按 `workspace` 过滤 + 条数上限）。
    fn list(&self, workspace: &Path, limit: usize, max_output_len: usize) -> String;
}
