//! `GET /health`、`GET /status`。

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;

use super::super::app_state::AppState;
use crate::agent::message_pipeline::MESSAGE_PIPELINE_COUNTERS;
use crate::chat_job_queue;
use crate::health;
use crate::tool_registry;

pub(crate) async fn health_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let work_dir = std::path::PathBuf::from(state.effective_workspace_path().await);
    let (auth_mode, probe, probe_cache_secs, api_base) = {
        let g = state.cfg.read().await;
        (
            g.llm_http_auth_mode,
            g.health_llm_models_probe,
            g.health_llm_models_probe_cache_secs,
            g.api_base.clone(),
        )
    };
    let mut report = health::build_health_report(&work_dir, &state.api_key, auth_mode, true).await;
    health::append_llm_models_endpoint_probe(
        &mut report,
        health::LlmModelsEndpointProbeParams {
            enabled: probe,
            cache_secs: probe_cache_secs,
            cache_cell: state.llm_models_health_cache.as_ref(),
            client: &state.client,
            api_base: api_base.as_str(),
            api_key: state.api_key.as_str(),
            auth_mode,
        },
    )
    .await;
    Json(report)
}

#[derive(serde::Serialize)]
struct StatusResponse {
    status: &'static str,
    model: String,
    api_base: String,
    max_tokens: u32,
    temperature: f32,
    /// 默认写入 `chat/completions` 的整数 seed（未配置则为 `null`）。
    llm_seed: Option<i64>,
    /// 当前加载进 API 请求的工具定义数量（`--no-tools` 时为 0）。
    tool_count: usize,
    /// 与模型对话时实际下发的工具名列表。
    tool_names: Vec<String>,
    /// `tool_registry` 中显式声明的分发策略（其余名称运行时走同步 `run_tool`）。
    tool_dispatch_registry: &'static [tool_registry::ToolDispatchMeta],
    reflection_default_max_rounds: usize,
    final_plan_requirement: crate::agent::per_coord::FinalPlanRequirementMode,
    plan_rewrite_max_attempts: usize,
    /// 规划器/执行器模式：single_agent | logical_dual_agent。
    planner_executor_mode: &'static str,
    /// 为 true 时每条用户消息先无工具规划轮再按步执行（见 `agent::agent_turn`）。
    staged_plan_execution: bool,
    /// CLI 是否在分阶段/逻辑双 agent 的**无工具规划轮**向 stdout 打印模型原文（默认 true）。
    staged_plan_cli_show_planner_stream: bool,
    /// 首轮规划后是否再跑无工具「步骤优化」轮（默认 true）。
    staged_plan_optimizer_round: bool,
    /// 逻辑多规划员份数上限（1–3，默认 1 即关闭）。
    staged_plan_ensemble_count: u8,
    /// SyncDefault 工具沙盒：`none` | `docker`。
    sync_default_tool_sandbox_mode: String,
    /// `docker` 模式下的镜像名（可能为空表示未启用或未配置）。
    sync_default_tool_sandbox_docker_image: String,
    /// Docker 沙盒容器进程身份摘要：`effective_uid:gid` | `image_default`（与配置 `current` / `image` 等对应）。
    sync_default_tool_sandbox_docker_user_effective: String,
    /// CLI REPL 是否在启动时从 `.crabmate/tui_session.json` 恢复会话（默认 false；文件名历史兼容）。
    tui_load_session_on_start: bool,
    /// CLI REPL 是否在后台构建 `initial_workspace_messages`（默认 false；仅 REPL）。
    repl_initial_workspace_messages_enabled: bool,
    max_message_history: usize,
    tool_message_max_chars: usize,
    context_char_budget: usize,
    context_summary_trigger_chars: usize,
    chat_queue_max_concurrent: usize,
    chat_queue_max_pending: usize,
    parallel_readonly_tools_max: usize,
    /// 单轮 `read_file` 缓存容量；`0` 表示关闭。
    read_file_turn_cache_max_entries: usize,
    chat_queue_running: usize,
    chat_queue_completed_ok: u64,
    chat_queue_completed_cancelled: u64,
    chat_queue_completed_err: u64,
    chat_queue_recent_jobs: Vec<chat_job_queue::ChatJobRecord>,
    /// 队列中正在执行的 `/chat`、`/chat/stream` 任务之 PER 镜像（无任务或无非队列调用时为空）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    per_active_jobs: Vec<chat_job_queue::PerFlightStatusEntry>,
    /// Web `POST /workspace` 允许的工作区根目录个数（未配置 `workspace_allowed_roots` 时为 1，即仅 `run_command_working_dir`）。
    workspace_allowed_roots_count: usize,
    /// 当前内存会话存储中的会话数量（按 `conversation_id`）。
    conversation_store_entries: usize,
    /// 长期记忆是否启用（配置）。
    long_term_memory_enabled: bool,
    /// 向量后端：`disabled` / `fastembed` 等。
    long_term_memory_vector_backend: String,
    /// 本进程是否已挂载记忆运行时（含与会话库共用 SQLite 或独立库路径）。
    long_term_memory_store_ready: bool,
    /// 异步索引累计失败次数（成功回合不递增；仅排障用）。
    long_term_memory_index_errors: u64,
    /// Web 新会话首轮是否注入自动生成的项目画像 Markdown。
    project_profile_inject_enabled: bool,
    /// 项目画像注入正文最大字符数（0 表示关闭生成）。
    project_profile_inject_max_chars: usize,
    /// 首轮是否追加 `cargo metadata` + package.json 的结构化摘要与 Mermaid workspace 图。
    project_dependency_brief_inject_enabled: bool,
    project_dependency_brief_inject_max_chars: usize,
    /// 是否要求非只读工具在 JSON 中带 `crabmate_explain_why`。
    tool_call_explain_enabled: bool,
    tool_call_explain_min_chars: usize,
    tool_call_explain_max_chars: usize,
    /// 自进程启动以来，同步上下文管道实际触发次数（累计，供排障；非「当前会话」）。
    message_pipeline_trim_count_hits: u64,
    message_pipeline_trim_char_budget_hits: u64,
    message_pipeline_tool_compress_hits: u64,
    message_pipeline_orphan_tool_drops: u64,
    /// 模型 HTTP 鉴权：`bearer` | `none`（如本地 Ollama 可不设 API_KEY）。
    llm_http_auth_mode: &'static str,
    /// 配置中的命名角色 id 列表（升序）；未启用多角色时为空。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    agent_role_ids: Vec<String>,
    /// Web/CLI 未指定 `agent_role` 时使用的默认角色 id（`null` 表示用全局 `system_prompt`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    default_agent_role_id: Option<String>,
}

pub(crate) async fn status_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let cfg = state.cfg.read().await;
    let mp = MESSAGE_PIPELINE_COUNTERS.snapshot();
    let conversation_store_entries = state.conversation_count().await;
    let (ltm_ready, ltm_idx_err) = match state.long_term_memory.as_ref() {
        Some(l) => (
            true,
            l.index_errors.load(std::sync::atomic::Ordering::Relaxed),
        ),
        None => (false, 0u64),
    };
    let tool_names: Vec<String> = state
        .tools
        .iter()
        .map(|t| t.function.name.clone())
        .collect();
    let mut agent_role_ids: Vec<String> = cfg.agent_roles.keys().cloned().collect();
    agent_role_ids.sort();
    Json(StatusResponse {
        status: "ok",
        model: cfg.model.clone(),
        api_base: cfg.api_base.clone(),
        max_tokens: cfg.max_tokens,
        temperature: cfg.temperature,
        llm_seed: cfg.llm_seed,
        tool_count: tool_names.len(),
        tool_names,
        tool_dispatch_registry: tool_registry::all_dispatch_metadata(),
        reflection_default_max_rounds: cfg.reflection_default_max_rounds,
        final_plan_requirement: cfg.final_plan_requirement,
        plan_rewrite_max_attempts: cfg.plan_rewrite_max_attempts,
        planner_executor_mode: cfg.planner_executor_mode.as_str(),
        staged_plan_execution: cfg.staged_plan_execution,
        staged_plan_cli_show_planner_stream: cfg.staged_plan_cli_show_planner_stream,
        staged_plan_optimizer_round: cfg.staged_plan_optimizer_round,
        staged_plan_ensemble_count: cfg.staged_plan_ensemble_count,
        sync_default_tool_sandbox_mode: cfg.sync_default_tool_sandbox_mode.as_str().to_string(),
        sync_default_tool_sandbox_docker_image: cfg.sync_default_tool_sandbox_docker_image.clone(),
        sync_default_tool_sandbox_docker_user_effective: match cfg
            .sync_default_tool_sandbox_docker_user
            .as_docker_user_string()
        {
            Some(s) => s.to_string(),
            None => "image_default".to_string(),
        },
        tui_load_session_on_start: cfg.tui_load_session_on_start,
        repl_initial_workspace_messages_enabled: cfg.repl_initial_workspace_messages_enabled,
        max_message_history: cfg.max_message_history,
        tool_message_max_chars: cfg.tool_message_max_chars,
        context_char_budget: cfg.context_char_budget,
        context_summary_trigger_chars: cfg.context_summary_trigger_chars,
        chat_queue_max_concurrent: state.chat_queue.max_concurrent(),
        chat_queue_max_pending: state.chat_queue.max_pending(),
        parallel_readonly_tools_max: cfg.parallel_readonly_tools_max,
        read_file_turn_cache_max_entries: cfg.read_file_turn_cache_max_entries,
        chat_queue_running: state.chat_queue.running_count(),
        chat_queue_completed_ok: state.chat_queue.completed_ok(),
        chat_queue_completed_cancelled: state.chat_queue.completed_cancelled(),
        chat_queue_completed_err: state.chat_queue.completed_err(),
        chat_queue_recent_jobs: state.chat_queue.recent_jobs(),
        per_active_jobs: state.chat_queue.active_per_jobs(),
        workspace_allowed_roots_count: cfg.workspace_allowed_roots.len(),
        conversation_store_entries,
        long_term_memory_enabled: cfg.long_term_memory_enabled,
        long_term_memory_vector_backend: cfg.long_term_memory_vector_backend.as_str().to_string(),
        long_term_memory_store_ready: ltm_ready,
        long_term_memory_index_errors: ltm_idx_err,
        project_profile_inject_enabled: cfg.project_profile_inject_enabled,
        project_profile_inject_max_chars: cfg.project_profile_inject_max_chars,
        project_dependency_brief_inject_enabled: cfg.project_dependency_brief_inject_enabled,
        project_dependency_brief_inject_max_chars: cfg.project_dependency_brief_inject_max_chars,
        tool_call_explain_enabled: cfg.tool_call_explain_enabled,
        tool_call_explain_min_chars: cfg.tool_call_explain_min_chars,
        tool_call_explain_max_chars: cfg.tool_call_explain_max_chars,
        message_pipeline_trim_count_hits: mp.trim_count_hits,
        message_pipeline_trim_char_budget_hits: mp.trim_char_budget_hits,
        message_pipeline_tool_compress_hits: mp.tool_compress_hits,
        message_pipeline_orphan_tool_drops: mp.orphan_tool_drops,
        llm_http_auth_mode: cfg.llm_http_auth_mode.as_str(),
        agent_role_ids,
        default_agent_role_id: cfg.default_agent_role_id.clone(),
    })
}
