use futures_util::stream::{self, StreamExt};
use log::{info, warn};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tracing::Instrument;

use crate::agent::agent_turn::sub_agent_policy::{
    executor_kind_tool_denied_body, tool_allowed_for_step_executor_kind,
};
use crate::agent_role_turn::{tool_allowed_for_turn, turn_tool_denied_message};
use crate::config::AgentConfig;
use crate::memory::long_term_memory::LongTermMemoryRuntime;
use crate::tool_registry::{
    self, CliToolRuntime, HandlerId, HandlerLookupTable, ToolRuntime, WebToolRuntime,
};
use crate::tool_result::ToolEnvelopeContext;
use crate::types::{Message, ToolCall};
use crate::workspace::changelist::WorkspaceChangelist;

use super::ExecuteToolsBatchOutcome;
use super::{
    ExecuteToolsCommonCtx, PARALLEL_READONLY_TOOL_BATCH_SEQ, abort_tool_batch_if_sse_closed,
    dedup_readonly_tool_calls_count, emit_timeline_log_sse, emit_tool_call_summary_sse,
    emit_tool_result_sse_and_append, trace_parallel_tool_child_span,
};

/// 并行执行时工具的分类，用于在构建 fut 前预分类，消除 if/else if/else 字符串比较。
#[derive(Clone, Copy)]
enum ParallelToolKind {
    HttpFetch,
    GetWeather,
    WebSearch,
    SyncDefault,
}

/// 只读并行路径独有字段（与 [`ExecuteToolsCommonCtx`] 对齐切片）。
struct ParallelUniqueBatchParts<'a> {
    tool_calls: &'a [ToolCall],
    cfg: &'a Arc<AgentConfig>,
    effective_working_dir: &'a std::path::Path,
    read_file_turn_cache: Option<Arc<crate::read_file_turn_cache::ReadFileTurnCache>>,
    workspace_changelist: Option<&'a Arc<WorkspaceChangelist>>,
    web_tool_ctx: Option<&'a WebToolRuntime>,
    cli_tool_ctx: Option<&'a CliToolRuntime>,
    step_executor_constraint: Option<crate::agent::plan_artifact::PlanStepExecutorKind>,
    tools_defs_hint: Arc<Vec<crate::types::Tool>>,
    turn_allow: Option<&'a HashSet<String>>,
    long_term_memory: Option<Arc<LongTermMemoryRuntime>>,
    long_term_memory_scope_id: Option<String>,
    tracing_chat_turn: Option<Arc<crate::observability::TracingChatTurn>>,
    handler_lookup: HandlerLookupTable,
    sync_default_sandbox_backend: Arc<dyn crate::tool_sandbox::SyncDefaultSandboxBackend>,
}

fn parallel_readonly_log_batch_start(
    tool_calls: &[ToolCall],
    dedup_count: usize,
    parallel_max: usize,
) {
    info!(
        target: super::LOG_TARGET,
        "并行执行工具批 count={} unique={} max_parallel={}（只读 SyncDefault + http_fetch + get_weather + web_search；构建锁类除外）",
        tool_calls.len(),
        dedup_count,
        parallel_max
    );
}

fn parallel_readonly_turn_replay_batch(
    tool_calls: &[ToolCall],
    parallel_batch_id: &str,
    dedup_count: usize,
    parallel_max: usize,
) {
    crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
        "tool_batch_started",
        "parallel_readonly_batch",
        Some(&serde_json::json!({
            "phase": "tool_execution",
            "execution_mode": "parallel_readonly_batch",
            "parallel_batch_id": parallel_batch_id,
            "tool_call_count": tool_calls.len(),
            "dedup_count": dedup_count,
            "parallel_max": parallel_max,
        })),
    );
}

async fn parallel_prefetch_approval_failures(
    tool_calls: &[ToolCall],
    cfg: &Arc<AgentConfig>,
    web_tool_ctx: Option<&WebToolRuntime>,
    cli_tool_ctx: Option<&CliToolRuntime>,
    handler_lookup: &HandlerLookupTable,
) -> HashMap<(String, String), String> {
    let mut prefetch_failures = HashMap::new();
    if tool_calls.iter().any(|t| t.function.name == "http_fetch") {
        prefetch_failures.extend(
            tool_registry::prefetch_http_fetch_parallel_approvals(
                tool_calls,
                cfg,
                web_tool_ctx,
                cli_tool_ctx,
            )
            .await,
        );
    }
    prefetch_failures.extend(
        tool_registry::prefetch_parallel_syncdefault_approvals(
            tool_calls,
            web_tool_ctx,
            cli_tool_ctx,
            handler_lookup,
        )
        .await,
    );
    prefetch_failures
}

async fn parallel_collect_unique_results(
    parts: ParallelUniqueBatchParts<'_>,
    prefetch_failures: HashMap<(String, String), String>,
    parallel_max: usize,
) -> HashMap<(String, String), String> {
    let ParallelUniqueBatchParts {
        tool_calls,
        cfg,
        effective_working_dir,
        read_file_turn_cache,
        workspace_changelist,
        web_tool_ctx,
        cli_tool_ctx,
        step_executor_constraint,
        tools_defs_hint,
        turn_allow,
        long_term_memory,
        long_term_memory_scope_id,
        tracing_chat_turn,
        handler_lookup,
        sync_default_sandbox_backend,
    } = parts;

    let wd_root = effective_working_dir.to_path_buf();
    let mut seen_keys: HashSet<(String, String)> = HashSet::with_capacity(tool_calls.len());
    let mut unique_futs = Vec::new();
    for tc in tool_calls {
        let key = (tc.function.name.clone(), tc.function.arguments.clone());
        if !seen_keys.insert(key.clone()) {
            continue;
        }
        let wd = wd_root.clone();
        let prefetch_err = prefetch_failures.get(&key).cloned();
        let cfg = Arc::clone(cfg);
        let tools_defs_hint = Arc::clone(&tools_defs_hint);
        let rfc = read_file_turn_cache.clone();
        let wcl = workspace_changelist.cloned();
        let ltm = long_term_memory.clone();
        let ltm_scope = long_term_memory_scope_id.clone();
        let name = tc.function.name.clone();
        let args = tc.function.arguments.clone();
        let tc_owned = tc.clone();
        let tool_call_id_for_trace = tc.id.clone();
        let tracing_turn_parallel = tracing_chat_turn.clone();
        let hl = handler_lookup.clone();
        let sb = Arc::clone(&sync_default_sandbox_backend);
        let kind = match hl.id_for(name.as_str()) {
            HandlerId::HttpFetch => ParallelToolKind::HttpFetch,
            HandlerId::GetWeather => ParallelToolKind::GetWeather,
            HandlerId::WebSearch => ParallelToolKind::WebSearch,
            _ => ParallelToolKind::SyncDefault,
        };
        let constraint = step_executor_constraint;
        unique_futs.push(async move {
            if let Some(err) = prefetch_err {
                return (name, args, err);
            }
            if let Some(k) = constraint
                && !tool_allowed_for_step_executor_kind(cfg.as_ref(), name.as_str(), k)
            {
                let denied = executor_kind_tool_denied_body(
                    cfg.as_ref(),
                    tools_defs_hint.as_slice(),
                    name.as_str(),
                    k,
                );
                return (name, args, denied);
            }
            if !tool_allowed_for_turn(name.as_str(), turn_allow) {
                let denied = turn_tool_denied_message(name.as_str());
                return (name, args, denied);
            }
            let wall_secs =
                crate::tool_registry::parallel_tool_wall_timeout_secs(cfg.as_ref(), name.as_str());
            let name_timeout = name.clone();
            let args_timeout = args.clone();
            let name_for_log = name.clone();
            let args_for_log = args.clone();
            let name_for_return = name.clone();
            let args_for_return = args.clone();
            let parallel_span = trace_parallel_tool_child_span(
                tracing_turn_parallel.as_ref(),
                &tool_call_id_for_trace,
            );
            let span_for_enter = parallel_span.clone();
            let work = async move {
                let _parallel_guard = span_for_enter.enter();
                info!(
                    target: super::LOG_TARGET,
                    "并行工具开始 tool={} args_preview={}",
                    name_for_log,
                    crate::redact::tool_arguments_preview_for_log(&args_for_log)
                );
                let t_tool = Instant::now();
                let result = match kind {
                    ParallelToolKind::HttpFetch => {
                        let span_http = tracing::Span::current();
                        tokio::task::spawn_blocking(move || {
                            let _g = span_http.enter();
                            let (mem_rt, mem_scope) =
                                crate::memory::long_term_memory::tool_context_memory_extras(
                                    cfg.as_ref(),
                                    ltm.clone(),
                                    ltm_scope.as_deref(),
                                );
                            let ctx = crate::tools::tool_context_for_with_read_cache_and_memory(
                                cfg.as_ref(),
                                cfg.command_exec.allowed_commands.as_ref(),
                                wd.as_path(),
                                rfc.as_ref().map(|a| a.as_ref()),
                                wcl.as_ref(),
                                mem_rt,
                                mem_scope,
                            );
                            crate::tools::http_fetch::run_direct(&args, &ctx)
                        })
                        .await
                        .unwrap_or_else(|e| format!("工具执行 panic：{}", e))
                    }
                    ParallelToolKind::GetWeather
                    | ParallelToolKind::WebSearch
                    | ParallelToolKind::SyncDefault => {
                        let mut workspace_changed_local = false;
                        let runtime = if let Some(cctx) = cli_tool_ctx {
                            ToolRuntime::Cli {
                                workspace_changed: &mut workspace_changed_local,
                                ctx: cctx,
                            }
                        } else {
                            ToolRuntime::Web {
                                workspace_changed: &mut workspace_changed_local,
                                ctx: web_tool_ctx,
                            }
                        };
                        tool_registry::dispatch_tool(tool_registry::DispatchToolParams {
                            runtime,
                            cfg: &cfg,
                            effective_working_dir: wd.as_path(),
                            workspace_is_set: true,
                            name: &name,
                            args: &args,
                            sse_out_tx: None,
                            sse_control_mirror: None,
                            tc: &tc_owned,
                            read_file_turn_cache: rfc.clone(),
                            workspace_changelist: wcl.clone(),
                            mcp_session: None,
                            turn_allow,
                            long_term_memory: ltm.clone(),
                            long_term_memory_scope_id: ltm_scope.clone(),
                            handler_lookup: &hl,
                            sync_default_sandbox_backend: &sb,
                        })
                        .await
                        .0
                    }
                };
                info!(
                    target: super::LOG_TARGET,
                    "并行工具完成 tool={} args_preview={} elapsed_ms={}",
                    name_for_log,
                    crate::redact::tool_arguments_preview_for_log(&args_for_return),
                    t_tool.elapsed().as_millis()
                );
                (name_for_return, args_for_return, result)
            }
            .instrument(parallel_span);
            match tokio::time::timeout(Duration::from_secs(wall_secs), work).await {
                Ok(triple) => triple,
                Err(_) => {
                    warn!(
                        target: super::LOG_TARGET,
                        "并行工具墙上时钟超时 tool={} args_preview={} wall_secs={}",
                        name_timeout,
                        crate::redact::tool_arguments_preview_for_log(&args_timeout),
                        wall_secs
                    );
                    (
                        name_timeout,
                        args_timeout,
                        format!("工具执行超时（{} 秒）", wall_secs),
                    )
                }
            }
        });
    }
    let unique_outcomes: Vec<(String, String, String)> = stream::iter(unique_futs)
        .buffer_unordered(parallel_max)
        .collect()
        .await;
    unique_outcomes
        .into_iter()
        .map(|(n, a, r)| ((n, a), r))
        .collect()
}

struct ParallelEmitOrderedParams<'a> {
    tool_calls: &'a [ToolCall],
    result_by_name_args: &'a HashMap<(String, String), String>,
    parallel_batch_id_ref: &'a str,
    per_coord: &'a mut crate::agent::per_coord::PerCoordinator,
    messages: &'a mut Vec<Message>,
    cfg: &'a Arc<AgentConfig>,
    tool_outcome_recorder: &'a Arc<crate::tool_stats::ToolOutcomeRecorder>,
    out: Option<&'a tokio::sync::mpsc::Sender<String>>,
    sse_control_mirror: Option<crate::sse::SseControlMirror>,
    clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    echo_terminal_transcript: bool,
    terminal_tool_display_max_chars: usize,
    tool_result_envelope_v1: bool,
}

async fn parallel_emit_ordered_tool_results(
    p: ParallelEmitOrderedParams<'_>,
) -> ExecuteToolsBatchOutcome {
    let ParallelEmitOrderedParams {
        tool_calls,
        result_by_name_args,
        parallel_batch_id_ref,
        per_coord,
        messages,
        cfg,
        tool_outcome_recorder,
        out,
        sse_control_mirror,
        clarification_questionnaire_hook,
        echo_terminal_transcript,
        terminal_tool_display_max_chars,
        tool_result_envelope_v1,
    } = p;

    let sse_mirror_ref = sse_control_mirror.as_ref();

    for tc in tool_calls {
        if abort_tool_batch_if_sse_closed(
            out,
            "SSE sender closed during parallel tool batch, aborting remainder",
        )
        .await
        {
            return ExecuteToolsBatchOutcome::AbortedSse;
        }
        emit_tool_call_summary_sse(
            out,
            sse_mirror_ref,
            cfg.as_ref(),
            tc.id.as_str(),
            &tc.function.name,
            &tc.function.arguments,
            messages,
        )
        .await;
        emit_timeline_log_sse(
            out,
            sse_mirror_ref,
            "tool_step_started",
            tc.function.name.clone(),
            Some(format!(
                "args={}",
                crate::redact::tool_arguments_preview_for_sse(&tc.function.arguments)
            )),
            "execute_tools::timeline tool_step_started",
        )
        .await;
        let key = (tc.function.name.clone(), tc.function.arguments.clone());
        let cached = result_by_name_args
            .get(&key)
            .map(String::as_str)
            .unwrap_or("")
            .to_string();
        let env = ToolEnvelopeContext {
            tool_call_id: tc.id.as_str(),
            execution_mode: "parallel_readonly_batch",
            parallel_batch_id: Some(parallel_batch_id_ref),
        };
        emit_tool_result_sse_and_append(
            messages,
            per_coord,
            super::EmitToolResultParams {
                cfg,
                tool_outcome_recorder,
                out,
                sse_control_mirror: sse_control_mirror.clone(),
                clarification_questionnaire_hook: clarification_questionnaire_hook.clone(),
                echo_terminal_transcript,
                terminal_tool_display_max_chars,
                tool_result_envelope_v1,
                name: &tc.function.name,
                args: &tc.function.arguments,
                id: &tc.id,
                result: cached,
                reflection_inject: None,
                envelope_ctx: Some(env),
            },
        )
        .await;
    }
    ExecuteToolsBatchOutcome::Finished
}

/// 只读可并行批：去重后 `spawn_blocking` + 限并发，再按原 `tool_calls` 顺序回写 SSE / messages。
pub(super) async fn execute_tools_parallel(
    ctx: ExecuteToolsCommonCtx<'_>,
) -> ExecuteToolsBatchOutcome {
    let ExecuteToolsCommonCtx {
        tool_calls,
        per_coord,
        messages,
        cfg,
        effective_working_dir,
        workspace_is_set: _,
        read_file_turn_cache,
        workspace_changelist,
        out,
        tool_running_hook: _,
        echo_terminal_transcript,
        terminal_tool_display_max_chars,
        tool_result_envelope_v1,
        web_tool_ctx,
        cli_tool_ctx,
        mcp_session: _,
        request_chrome_trace: _,
        step_executor_constraint,
        tools_defs_full,
        turn_allow,
        long_term_memory,
        long_term_memory_scope_id,
        tracing_chat_turn,
        request_audit: _,
        tool_outcome_recorder,
        handler_lookup,
        sync_default_sandbox_backend,
        readonly_tool_ttl_cache: _,
        clarification_questionnaire_hook,
        sse_control_mirror,
    } = ctx;

    let sse_mirror_parallel = sse_control_mirror.clone();

    let sandbox_backend = Arc::clone(&sync_default_sandbox_backend);

    let tools_defs_hint = Arc::new(tools_defs_full.to_vec());

    let dedup_count = dedup_readonly_tool_calls_count(tool_calls);
    let parallel_max = cfg.chat_queues_cache.parallel_readonly_tools_max.max(1);
    parallel_readonly_log_batch_start(tool_calls, dedup_count, parallel_max);

    let parallel_batch_id = format!(
        "prb-{}",
        PARALLEL_READONLY_TOOL_BATCH_SEQ.fetch_add(1, Ordering::Relaxed)
    );
    parallel_readonly_turn_replay_batch(
        tool_calls,
        parallel_batch_id.as_str(),
        dedup_count,
        parallel_max,
    );
    let parallel_batch_id_ref = parallel_batch_id.as_str();

    let prefetch_failures = parallel_prefetch_approval_failures(
        tool_calls,
        cfg,
        web_tool_ctx,
        cli_tool_ctx,
        &handler_lookup,
    )
    .await;

    let result_by_name_args = parallel_collect_unique_results(
        ParallelUniqueBatchParts {
            tool_calls,
            cfg,
            effective_working_dir,
            read_file_turn_cache,
            workspace_changelist,
            web_tool_ctx,
            cli_tool_ctx,
            step_executor_constraint,
            tools_defs_hint,
            turn_allow,
            long_term_memory,
            long_term_memory_scope_id,
            tracing_chat_turn,
            handler_lookup,
            sync_default_sandbox_backend: sandbox_backend.clone(),
        },
        prefetch_failures,
        parallel_max,
    )
    .await;

    parallel_emit_ordered_tool_results(ParallelEmitOrderedParams {
        tool_calls,
        result_by_name_args: &result_by_name_args,
        parallel_batch_id_ref,
        per_coord,
        messages,
        cfg,
        tool_outcome_recorder: &tool_outcome_recorder,
        out,
        sse_control_mirror: sse_mirror_parallel,
        echo_terminal_transcript,
        terminal_tool_display_max_chars,
        tool_result_envelope_v1,
        clarification_questionnaire_hook: clarification_questionnaire_hook.clone(),
    })
    .await
}
