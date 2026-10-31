//! 记忆相关工具宿主（由 `cm_internal` 注入，避免 `cm_tools` → `cm_memory` 环）。

use std::path::Path;

use crate::cm_config::AgentConfig;
use crate::cm_tools::tool_result::ToolError;

/// `codebase_semantic_search` 执行面。
///
/// 返回 [`ToolError`] 而非纯文本：索引库打不开、`semantic_only` 缺 `fastembed` 构建、
/// 参数 JSON 坏都必须显式透传（历史实现里 `参数 JSON 无效: …` 与
/// `index_path_for_workspace` 的错误串都没有「错误：」前缀，会被启发式判成**成功**）。
pub trait CodebaseSemanticToolHost: Send + Sync {
    #[allow(clippy::result_large_err)] // `ToolError` 含正文解析快照（`parsed`），与工具 runner 一致
    fn run_search(
        &self,
        args_json: &str,
        working_dir: &Path,
        max_output_len: usize,
    ) -> Result<String, ToolError>;
}

/// `long_term_*` / `summarize_experience` 执行面。
///
/// 返回 [`ToolError`] 而非纯文本：这些工具的失败（参数 JSON 无效、内容为空/过短、
/// 记忆库 IO 失败）须显式透传，不能靠正文关键词推断（历史实现里 `参数 JSON 无效: …`
/// 没有「错误：」前缀，会被启发式判成**成功**）。
pub trait LongTermMemoryToolHost: Send + Sync {
    #[allow(clippy::result_large_err)] // `ToolError` 含正文解析快照（`parsed`），与工具 runner 一致
    fn dispatch(
        &self,
        tool_name: &str,
        args_json: &str,
        cfg: &AgentConfig,
    ) -> Result<String, ToolError>;
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
    /// 增量拉取输出（`tail -f` 语义）；`cursor` 省略=从最早可用起。
    fn output(&self, id: &str, cursor: Option<u64>, max_output_len: usize) -> String;
    /// 取消任务（`queued` 直接转移；`running` 置取消标记；终态不覆盖）。
    fn cancel(&self, id: &str) -> String;
    /// 阻塞等待任务满足条件（`background_job_wait`）：**不改变任务状态**。
    ///
    /// - `pattern` 为 `None` → 等终态；`Some(p)` → 等终态 **或** 输出（保留窗口内）出现子串 `p`。
    /// - `timeout_secs` 由调用方钳制（已为外圈墙钟留出余量），实现侧按此墙钟返回即止。
    fn wait_until(
        &self,
        id: &str,
        pattern: Option<&str>,
        timeout_secs: u64,
        max_output_len: usize,
    ) -> String;
}