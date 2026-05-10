//! 热重载时把新配置的可变子集合并进运行中的 [`super::types::AgentConfig`]。

use super::types::AgentConfig;
/// 将 **`load_config` 新结果** 中的「可热更」字段写入 `dst`，保留 **`dst` 中需进程级冻结的项**。
///
/// ## 边界（REPL **`/config reload`** / Web **`POST /config/reload`**）
///
/// - **`API_KEY`**：仍来自**进程环境**；本函数**不**读取或改写密钥，与启动时一致。
/// - **`conversation_store_sqlite_path`**：**不**热更（会话 SQLite 连接在启动时打开；改路径须重启 `serve`）。
/// - **`api_base` / `model` / `llm_http_auth_mode`**：从磁盘+环境变量**重新应用**（与 [`load_config`] 一致），**下一轮** LLM 请求起生效；共享 `reqwest::Client` 的连接池可能短暂保留旧主机空闲连接，直至池超时。
/// - **`scheduled_agent_tasks`**：热更可更新内存中的列表；**cron 注册仅在 `serve` 启动时完成**，改表达式或增减任务需重启进程生效。
/// - **`web_api_require_bearer`**：热更后字段与 **`web_api_bearer_token`** 一并更新；**`true`** 时与「非空密钥」的**启动级**强制组合仅在下次 **`serve`** 启动时校验（中间件是否挂载仍仅由启动时 token 是否非空决定）。
/// - **`system_prompt`**（含 **`system_prompt_file`** 重读）：从 `src` 写入，下一轮起生效。
/// - **`agent_tool_stats_*`**：热更后影响**下一轮起**附加段内容；已打开会话的 `system` 不会自动改写。
/// - **MCP**：`mcp_enabled` / `mcp_command` / `mcp_tool_timeout_secs` 会更新；调用方应在提交前 [`crate::mcp::clear_mcp_process_cache`].
pub fn apply_hot_reload_config_subset(dst: &mut AgentConfig, src: &AgentConfig) {
    dst.llm.clone_from(&src.llm);
    dst.session_ui.clone_from(&src.session_ui);
    dst.command_exec.clone_from(&src.command_exec);
    dst.llm_sampling.clone_from(&src.llm_sampling);
    dst.llm_vendor_flags.clone_from(&src.llm_vendor_flags);
    dst.llm_http_retry.clone_from(&src.llm_http_retry);
    dst.weather_tool.clone_from(&src.weather_tool);
    dst.web_search.clone_from(&src.web_search);
    dst.http_fetch.clone_from(&src.http_fetch);
    dst.per_plan_policy.clone_from(&src.per_plan_policy);
    dst.roles_prompts.clone_from(&src.roles_prompts);
    dst.cursor_rules.clone_from(&src.cursor_rules);
    dst.skills.clone_from(&src.skills);
    dst.tool_transcript.clone_from(&src.tool_transcript);
    dst.agent_thinking_trace
        .clone_from(&src.agent_thinking_trace);
    dst.agent_tool_stats.clone_from(&src.agent_tool_stats);
    dst.dsml_materialize.clone_from(&src.dsml_materialize);
    dst.thinking_echo.clone_from(&src.thinking_echo);
    dst.context_pipeline.clone_from(&src.context_pipeline);
    dst.workspace_roots.clone_from(&src.workspace_roots);
    dst.web_api.clone_from(&src.web_api);
    dst.chat_queues_cache.clone_from(&src.chat_queues_cache);
    dst.session_workspace_changelist
        .clone_from(&src.session_workspace_changelist);
    dst.staged_planning.clone_from(&src.staged_planning);
    dst.sync_tool_sandbox.clone_from(&src.sync_tool_sandbox);
    dst.context_bootstrap_inject
        .clone_from(&src.context_bootstrap_inject);
    dst.tool_call_explain.clone_from(&src.tool_call_explain);
    dst.long_term_memory.clone_from(&src.long_term_memory);
    dst.mcp_client.clone_from(&src.mcp_client);
    dst.codebase_semantic.clone_from(&src.codebase_semantic);
    dst.tool_registry_policy
        .clone_from(&src.tool_registry_policy);
    dst.turn_budget.clone_from(&src.turn_budget);
    dst.hierarchy_routing.clone_from(&src.hierarchy_routing);
    dst.intent_routing.clone_from(&src.intent_routing);

    dst.conversation_persistence.scheduled_agent_tasks =
        src.conversation_persistence.scheduled_agent_tasks.clone();
}
