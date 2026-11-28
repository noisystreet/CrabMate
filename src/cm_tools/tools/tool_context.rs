//! 工具执行上下文（`ToolContext`）与顶层分类（`ToolCategory`），含构造器。
//!
//! `#1530`：从 `tools/mod.rs` 拆出。命名空间不变——`tools/mod.rs` 以
//! `pub use tool_context::*;` 承接，故 `crate::cm_tools::tools::ToolContext` 等路径保持原样。

use std::sync::Arc;

use crate::cm_config::{AgentConfig, ExposeSecret};
use crate::cm_tools::memory_tool_host::{
    CodebaseSemanticToolHost, LongTermMemoryToolHost, ToolJobsToolHost,
};
use crate::cm_tools::workspace::changelist::WorkspaceChangelist;

/// 工具顶层分类（用于 `build_tools_filtered`、文档与后续按场景裁剪工具列表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// 基础工具：天气、联网搜索与受控 HTTP、文本/格式转换、日程与提醒等（不依赖「在仓库里写代码」）。
    Basic,
    /// 开发工具：工作区文件、Git、Cargo/前端构建与测试、Lint、补丁、符号搜索、工作流等。
    Development,
}

impl ToolCategory {
    /// 对外稳定枚举字面量：`basic` / `development`（供 `GET /tools` 投影）。
    pub fn as_str(self) -> &'static str {
        match self {
            ToolCategory::Basic => "basic",
            ToolCategory::Development => "development",
        }
    }
}

pub struct ToolContext<'a> {
    /// 主 Agent / `tool_context_for*` 路径填充；工作流节点等自建上下文可为 `None`（部分工具将报错）。
    pub cfg: Option<&'a AgentConfig>,
    /// 代码语义检索；主 Agent 路径由 `cm_internal` 注入。
    pub codebase_semantic_host: Option<&'a dyn CodebaseSemanticToolHost>,
    pub command_max_output_len: usize,
    pub weather_timeout_secs: u64,
    pub allowed_commands: &'a [String],
    pub working_dir: &'a std::path::Path,
    pub web_search_timeout_secs: u64,
    pub web_search_provider: crate::cm_config::WebSearchProvider,
    pub web_search_api_key: &'a str,
    pub web_search_max_results: u32,
    /// worbrow 浏览器/静态抓取代理（`http://host:port` / `https://host:port`；空 = 直连/系统代理）。
    /// 仅 `web_search_provider = worbrow` 时透传给 worbrow `Config::with_proxy`。
    pub web_search_proxy: &'a str,
    pub http_fetch_allowed_prefixes: &'a [String],
    pub http_fetch_timeout_secs: u64,
    pub http_fetch_max_response_bytes: usize,
    /// `run_command` / 部分子进程工具的墙上时钟上限（秒）；`python_snippet_run` 等复用。
    pub command_timeout_secs: u64,
    /// 单轮 `run_agent_turn` 内 `read_file` 缓存；`None` 表示关闭。
    pub read_file_turn_cache: Option<&'a crate::cm_tools::read_file_turn_cache::ReadFileTurnCache>,
    /// 本会话工作区变更集（按 `long_term_memory_scope_id`）；`None` 时不记录。
    pub workspace_changelist: Option<&'a Arc<WorkspaceChangelist>>,
    /// `cargo_test` / 部分 `run_command cargo test` 的进程内输出缓存。
    pub test_result_cache_enabled: bool,
    pub test_result_cache_max_entries: usize,
    /// 长期记忆工具；主 Agent 路径由 `cm_internal` 注入。
    pub long_term_memory_host: Option<&'a dyn LongTermMemoryToolHost>,
    /// 后台任务查询工具（`background_job_status` / `background_job_list`）；主 Agent 路径由 `cm_internal` 注入。
    pub tool_jobs_host: Option<&'a dyn ToolJobsToolHost>,
    /// 文件类工具的路径范围（S3）。默认 [`crate::cm_types::PathScope::SessionRoot`] = 现状；
    /// 主 Agent 路径由 `dispatch` 按权限等级投影覆盖（见 [`ToolContext::with_path_scope_and_roots`]）。
    pub path_scope: crate::cm_types::PathScope,
    /// 多根放行的根集合（S7）——`workspace_allowed_roots` 的**借用**视图。
    ///
    /// 仅在 `path_scope = AllowedRoots` 时被 [`file::PathAccess`] 读取；空切片 = fail-closed
    /// 退化为 `SessionRoot`（见 `cm_types::path_scope_for`）。
    ///
    /// 根集合刻意**不进** `PermissionCaps`：后者要保持纯值、可 golden 整比、可序列化
    /// （设计 §16.2），根集合由本载体沿途注入。
    pub allowed_roots: &'a [std::path::PathBuf],
}

impl<'a> ToolContext<'a> {
    /// 同时覆盖路径范围与允许根集合（S3 / S7 的唯一注入点）。
    ///
    /// 两者必须一起给：`AllowedRoots` 配空根会静默退化为 `SessionRoot`，只给其中一个
    /// 等于让配置看起来生效、实际没放行。
    #[must_use]
    pub fn with_path_scope_and_roots(
        mut self,
        scope: crate::cm_types::PathScope,
        allowed_roots: &'a [std::path::PathBuf],
    ) -> Self {
        self.path_scope = scope;
        self.allowed_roots = allowed_roots;
        self
    }

    /// 只读聚合工具（`docs_health_sweep` / `repo_overview`）的占位上下文：不联网、不跑命令、
    /// 无记忆与后台任务宿主，只保留工作区根与命令输出上限（#1239：此前两处各手抄 26 行字段）。
    #[must_use]
    pub fn stub(working_dir: &'a std::path::Path, command_max_output_len: usize) -> Self {
        Self {
            cfg: None,
            codebase_semantic_host: None,
            command_max_output_len,
            weather_timeout_secs: 0,
            allowed_commands: &[],
            working_dir,
            web_search_timeout_secs: 0,
            web_search_provider: crate::cm_config::WebSearchProvider::Brave,
            web_search_api_key: "",
            web_search_max_results: 0,
            web_search_proxy: "",
            http_fetch_allowed_prefixes: &[],
            http_fetch_timeout_secs: 0,
            http_fetch_max_response_bytes: 0,
            command_timeout_secs: 30,
            read_file_turn_cache: None,
            workspace_changelist: None,
            test_result_cache_enabled: false,
            test_result_cache_max_entries: 8,
            long_term_memory_host: None,
            tool_jobs_host: None,
            path_scope: crate::cm_types::PathScope::SessionRoot,
            allowed_roots: &[],
        }
    }
}

/// 由 [`AgentConfig`] 与当前工作目录、命令白名单构造工具上下文（供 `run_tool` 使用）。
pub fn tool_context_for<'a>(
    cfg: &'a AgentConfig,
    allowed_commands: &'a [String],
    working_dir: &'a std::path::Path,
) -> ToolContext<'a> {
    ToolContext {
        cfg: Some(cfg),
        codebase_semantic_host: None,
        command_max_output_len: cfg.command_exec.command_max_output_len,
        weather_timeout_secs: cfg.weather_tool.weather_timeout_secs,
        allowed_commands,
        working_dir,
        web_search_timeout_secs: cfg.web_search.web_search_timeout_secs,
        web_search_provider: cfg.web_search.web_search_provider,
        web_search_api_key: cfg.web_search.web_search_api_key.expose_secret(),
        web_search_max_results: cfg.web_search.web_search_max_results,
        web_search_proxy: cfg.web_search.web_search_proxy.as_str(),
        http_fetch_allowed_prefixes: cfg.http_fetch.http_fetch_allowed_prefixes.as_slice(),
        http_fetch_timeout_secs: cfg.http_fetch.http_fetch_timeout_secs,
        http_fetch_max_response_bytes: cfg.http_fetch.http_fetch_max_response_bytes,
        command_timeout_secs: cfg.command_exec.command_timeout_secs,
        read_file_turn_cache: None,
        workspace_changelist: None,
        test_result_cache_enabled: cfg.chat_queues_cache.test_result_cache_enabled,
        test_result_cache_max_entries: cfg.chat_queues_cache.test_result_cache_max_entries,
        long_term_memory_host: None,
        tool_jobs_host: None,
        path_scope: crate::cm_types::PathScope::SessionRoot,
        // S7：根集合随上下文借用传递；路径轴本身由 `dispatch` 按等级覆盖。
        allowed_roots: cfg.workspace_roots.workspace_allowed_roots.as_slice(),
    }
}

/// 在 [`tool_context_for`] 基础上挂载单轮 `read_file` 缓存、会话变更集与可选记忆/后台任务宿主。
#[allow(clippy::too_many_arguments)] // 逐项注入可选宿主（语义索引 / 长期记忆 / 后台任务），展开为结构体反而更绕
pub fn tool_context_for_with_read_cache_and_memory<'a>(
    cfg: &'a AgentConfig,
    allowed_commands: &'a [String],
    working_dir: &'a std::path::Path,
    read_file_turn_cache: Option<&'a crate::cm_tools::read_file_turn_cache::ReadFileTurnCache>,
    workspace_changelist: Option<&'a Arc<WorkspaceChangelist>>,
    codebase_semantic_host: Option<&'a dyn CodebaseSemanticToolHost>,
    long_term_memory_host: Option<&'a dyn LongTermMemoryToolHost>,
    tool_jobs_host: Option<&'a dyn ToolJobsToolHost>,
) -> ToolContext<'a> {
    ToolContext {
        read_file_turn_cache,
        workspace_changelist,
        codebase_semantic_host,
        long_term_memory_host,
        tool_jobs_host,
        ..tool_context_for(cfg, allowed_commands, working_dir)
    }
}
