//! E 步：执行 tool_calls（SSE/终端、并行只读批、串行带缓存）。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures_util::stream::{self, StreamExt};
use log::{debug, info, warn};
use tokio::sync::mpsc;

static PARALLEL_READONLY_TOOL_BATCH_SEQ: AtomicU64 = AtomicU64::new(1);

use crate::agent::per_coord::PerCoordinator;
use crate::agent::plan_artifact::PlanStepExecutorKind;
use crate::config::AgentConfig;
use crate::sse::{SsePayload, ToolCallSummary, ToolResultBody, encode_message};
use crate::tool_registry::{self, ToolRuntime};
use crate::tool_result::{self, NormalizedToolEnvelope, ToolEnvelopeContext, parse_legacy_output};
use crate::tools;
use crate::types::{Message, ToolCall};
use crate::workspace_changelist::WorkspaceChangelist;

use super::sub_agent_policy::tool_allowed_for_step_executor_kind;

/// 本模块 `tracing` / `log` 的 `target`，便于 `RUST_LOG=crabmate::execute_tools` 过滤。
const LOG_TARGET: &str = "crabmate::execute_tools";

/// 并行执行时工具的分类，用于在构建 fut 前预分类，消除 if/else if/else 字符串比较。
#[derive(Clone, Copy)]
enum ParallelToolKind {
    HttpFetch,
    SyncDefault,
}

pub(crate) struct WebExecuteCtx<'a> {
    pub cfg: &'a Arc<AgentConfig>,
    pub effective_working_dir: &'a Path,
    pub workspace_is_set: bool,
    /// 单轮 `read_file` 缓存；`None` 表示关闭。
    pub read_file_turn_cache: Option<Arc<crate::read_file_turn_cache::ReadFileTurnCache>>,
    pub out: Option<&'a mpsc::Sender<String>>,
    pub web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    /// 终端 CLI：`run_command` 非白名单时 stdin 审批；`None` 时与历史一致（非白名单则无法执行）。
    pub cli_tool_ctx: Option<&'a tool_registry::CliToolRuntime>,
    /// CLI：`render_to_terminal` 且 `out: None` 时为 true，工具结果打印到 stdout。
    pub echo_terminal_transcript: bool,
    /// MCP stdio 会话；`None` 时 `mcp__*` 工具会报错。
    pub mcp_session: Option<&'a std::sync::Arc<tokio::sync::Mutex<crate::mcp::McpClientSession>>>,
    pub workspace_changelist: Option<&'a Arc<WorkspaceChangelist>>,
    /// 整请求 Chrome trace；与 `workflow_execute` 合并写 `turn-*.json`。
    pub request_chrome_trace: Option<Arc<crate::request_chrome_trace::RequestTurnTrace>>,
    /// 分阶段规划当前步子代理约束；与 `RunLoopParams::step_executor_constraint` 同步。
    pub step_executor_constraint: Option<PlanStepExecutorKind>,
}

pub(crate) enum ExecuteToolsBatchOutcome {
    /// 本批工具跑完，继续外层循环
    Finished,
    /// SSE 在工具执行中断开
    AbortedSse,
}

/// 单工具：SSE / 终端回显 + 追加 `tool` 与可选反思 `user`（与串行路径一致）的入参。
struct EmitToolResultParams<'a> {
    cfg: &'a Arc<AgentConfig>,
    out: Option<&'a mpsc::Sender<String>>,
    echo_terminal_transcript: bool,
    terminal_tool_display_max_chars: usize,
    tool_result_envelope_v1: bool,
    name: &'a str,
    args: &'a str,
    id: &'a str,
    result: String,
    reflection_inject: Option<serde_json::Value>,
    envelope_ctx: Option<ToolEnvelopeContext<'a>>,
}

/// SSE：`SsePayload::ToolResult`（含 stdout/stderr、retryable、信封元数据）。
async fn emit_sse_tool_result(
    tx: &mpsc::Sender<String>,
    name: &str,
    result: &str,
    tool_summary: Option<String>,
    envelope_ctx: Option<ToolEnvelopeContext<'_>>,
) {
    let parsed = parse_legacy_output(name, result);
    let summary_for_norm = tool_summary
        .clone()
        .unwrap_or_else(|| format!("tool: {name}"));
    let norm = NormalizedToolEnvelope::from_tool_run(
        name,
        summary_for_norm,
        &parsed,
        result,
        envelope_ctx.as_ref(),
    );
    let stdout = if parsed.stdout.is_empty() {
        None
    } else {
        Some(parsed.stdout)
    };
    let stderr = if parsed.stderr.is_empty() {
        None
    } else {
        Some(parsed.stderr)
    };
    let _ = crate::sse::send_string_logged(
        tx,
        encode_message(SsePayload::ToolResult {
            tool_result: ToolResultBody {
                name: norm.name,
                result_version: norm.envelope_version,
                summary: tool_summary,
                output: result.to_string(),
                ok: Some(norm.ok),
                exit_code: norm.exit_code,
                error_code: norm.error_code.clone(),
                retryable: norm.retryable,
                tool_call_id: norm.tool_call_id,
                execution_mode: norm.execution_mode,
                parallel_batch_id: norm.parallel_batch_id,
                stdout,
                stderr,
            },
        }),
        "execute_tools::emit_tool_result_sse",
    )
    .await;
}

/// SSE：`SsePayload::ToolRunning`（`out` 为 `None` 时 no-op）。
async fn emit_sse_tool_running(
    out: Option<&mpsc::Sender<String>>,
    tool_running: bool,
    log_label: &'static str,
) {
    let Some(tx) = out else {
        return;
    };
    let _ = crate::sse::send_string_logged(
        tx,
        encode_message(SsePayload::ToolRunning { tool_running }),
        log_label,
    )
    .await;
}

async fn emit_tool_result_sse_and_append(
    messages: &mut Vec<Message>,
    per_coord: &mut PerCoordinator,
    p: EmitToolResultParams<'_>,
) {
    let EmitToolResultParams {
        cfg,
        out,
        echo_terminal_transcript,
        terminal_tool_display_max_chars,
        tool_result_envelope_v1,
        name,
        args,
        id,
        result,
        reflection_inject,
        envelope_ctx,
    } = p;
    let args_parsed: Option<serde_json::Value> = serde_json::from_str(args).ok();
    let tool_summary = if let Some(ref parsed) = args_parsed {
        tools::summarize_tool_call_parsed(name, parsed)
    } else {
        tools::summarize_tool_call(name, args)
    };

    crate::runtime::terminal_cli_transcript::echo_tool_result_transcript(
        echo_terminal_transcript,
        out.is_some(),
        name,
        args,
        tool_summary.as_deref(),
        result.as_str(),
        terminal_tool_display_max_chars,
    );

    if let Some(tx) = out {
        emit_sse_tool_result(
            tx,
            name,
            result.as_str(),
            tool_summary.clone(),
            envelope_ctx,
        )
        .await;
    }

    crate::tool_stats::record_tool_outcome(
        cfg.as_ref(),
        name,
        result.as_str(),
        tool_summary.clone(),
        envelope_ctx.as_ref(),
    );

    let content_for_model = if tool_result_envelope_v1 {
        let parsed = parse_legacy_output(name, &result);
        let summary_str = tool_summary
            .clone()
            .unwrap_or_else(|| format!("tool: {name}"));
        tool_result::encode_tool_message_envelope_v1(
            name,
            summary_str,
            &parsed,
            &result,
            envelope_ctx.as_ref(),
        )
    } else {
        result
    };

    PerCoordinator::append_tool_result_and_reflection(
        per_coord,
        messages,
        id.to_string(),
        content_for_model,
        reflection_inject,
    );
}

/// SSE 发送端已关闭（与外层 `run_agent_turn` 早退判断一致）。
pub(crate) fn sse_sender_closed(out: Option<&mpsc::Sender<String>>) -> bool {
    out.is_some_and(|tx| tx.is_closed())
}

async fn emit_tool_call_summary_sse(out: Option<&mpsc::Sender<String>>, name: &str, args: &str) {
    let Some(tx) = out else {
        return;
    };
    let args_parsed: Option<serde_json::Value> = serde_json::from_str(args).ok();
    let summary = if let Some(ref parsed) = args_parsed {
        tools::summarize_tool_call_parsed(name, parsed)
    } else {
        tools::summarize_tool_call(name, args)
    }
    .unwrap_or_else(|| format!("tool: {name}"));
    let _ = crate::sse::send_string_logged(
        tx,
        encode_message(SsePayload::ToolCall {
            tool_call: ToolCallSummary {
                name: name.to_string(),
                summary,
            },
        }),
        "execute_tools::tool_call summary",
    )
    .await;
}

/// 工具批处理中发现 SSE 已断开：记日志、尽力下发「工具轮结束」，返回 `true` 时应中止批处理。
async fn abort_tool_batch_if_sse_closed(
    out: Option<&mpsc::Sender<String>>,
    reason: &'static str,
) -> bool {
    if !sse_sender_closed(out) {
        return false;
    }
    info!(target: LOG_TARGET, "{reason}");
    emit_sse_tool_running(
        out,
        false,
        "execute_tools::abort_tool_batch tool_running false",
    )
    .await;
    true
}

/// 统计并行只读批次中去重后的唯一 `(name, args)` 数。
pub(crate) fn dedup_readonly_tool_calls_count(tool_calls: &[ToolCall]) -> usize {
    let mut seen: HashSet<(&str, &str)> = HashSet::with_capacity(tool_calls.len());
    for tc in tool_calls {
        seen.insert((tc.function.name.as_str(), tc.function.arguments.as_str()));
    }
    seen.len()
}

/// E：执行一批 tool 调用（Web/CLI 共用骨架），写入 tool / 反思 user，并发送 SSE 片段。
///
/// 同名同参数的只读工具在同一批次内去重：并行路径只执行唯一实例后映射回各 `tool_call_id`；
/// 串行路径维护本批次只读缓存，遇写操作时清空（写操作可能改变文件系统状态，使先前读取结果失效）。
struct ExecuteToolsCommonCtx<'a> {
    tool_calls: &'a [ToolCall],
    per_coord: &'a mut PerCoordinator,
    messages: &'a mut Vec<Message>,
    cfg: &'a Arc<AgentConfig>,
    effective_working_dir: &'a Path,
    workspace_is_set: bool,
    read_file_turn_cache: Option<Arc<crate::read_file_turn_cache::ReadFileTurnCache>>,
    workspace_changelist: Option<&'a Arc<WorkspaceChangelist>>,
    out: Option<&'a mpsc::Sender<String>>,
    echo_terminal_transcript: bool,
    terminal_tool_display_max_chars: usize,
    tool_result_envelope_v1: bool,
    web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    cli_tool_ctx: Option<&'a tool_registry::CliToolRuntime>,
    mcp_session: Option<&'a std::sync::Arc<tokio::sync::Mutex<crate::mcp::McpClientSession>>>,
    request_chrome_trace: Option<Arc<crate::request_chrome_trace::RequestTurnTrace>>,
    step_executor_constraint: Option<PlanStepExecutorKind>,
}

/// 只读可并行批：去重后 `spawn_blocking` + 限并发，再按原 `tool_calls` 顺序回写 SSE / messages。
async fn execute_tools_parallel(ctx: ExecuteToolsCommonCtx<'_>) -> ExecuteToolsBatchOutcome {
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
        echo_terminal_transcript,
        terminal_tool_display_max_chars,
        tool_result_envelope_v1,
        web_tool_ctx,
        cli_tool_ctx,
        mcp_session: _,
        request_chrome_trace: _,
        step_executor_constraint,
    } = ctx;

    let dedup_count = dedup_readonly_tool_calls_count(tool_calls);
    let parallel_max = cfg.parallel_readonly_tools_max.max(1);
    info!(
        target: LOG_TARGET,
        "并行执行工具批 count={} unique={} max_parallel={}（只读 SyncDefault + http_fetch + get_weather + web_search；构建锁类除外）",
        tool_calls.len(),
        dedup_count,
        parallel_max
    );

    let parallel_batch_id = format!(
        "prb-{}",
        PARALLEL_READONLY_TOOL_BATCH_SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let parallel_batch_id_ref = parallel_batch_id.as_str();

    let prefetch_failures = if tool_calls.iter().any(|t| t.function.name == "http_fetch") {
        tool_registry::prefetch_http_fetch_parallel_approvals(
            tool_calls,
            cfg,
            web_tool_ctx,
            cli_tool_ctx,
        )
        .await
    } else {
        HashMap::new()
    };

    let mut seen_keys: HashSet<(String, String)> = HashSet::with_capacity(tool_calls.len());
    let mut unique_futs = Vec::new();
    for tc in tool_calls {
        let key = (tc.function.name.clone(), tc.function.arguments.clone());
        if !seen_keys.insert(key.clone()) {
            continue;
        }
        let prefetch_err = prefetch_failures.get(&key).cloned();
        let cfg = Arc::clone(cfg);
        let wd = effective_working_dir.to_path_buf();
        let rfc = read_file_turn_cache.clone();
        let wcl = workspace_changelist.cloned();
        let name = tc.function.name.clone();
        let args = tc.function.arguments.clone();
        let kind = if name == "http_fetch" {
            ParallelToolKind::HttpFetch
        } else {
            ParallelToolKind::SyncDefault
        };
        let constraint = step_executor_constraint;
        unique_futs.push(async move {
            if let Some(err) = prefetch_err {
                return (name, args, err);
            }
            if let Some(k) = constraint
                && !tool_allowed_for_step_executor_kind(cfg.as_ref(), name.as_str(), k)
            {
                let denied = format!(
                    "工具「{}」不在本步子代理角色 {:?} 的允许列表内；请改用该步允许的只读/补丁/测试工具，或让规划器省略 executor_kind。",
                    name.as_str(),
                    k
                );
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
            let work = async move {
                info!(target: LOG_TARGET, "并行工具开始 tool={}", name_for_log);
                debug!(
                    target: LOG_TARGET,
                    "工具调用参数摘要 tool={} args_preview={}",
                    name_for_log,
                    crate::redact::tool_arguments_preview_for_log(&args_for_log)
                );
                let t_tool = Instant::now();
                let result = match kind {
                    ParallelToolKind::HttpFetch => tokio::task::spawn_blocking(move || {
                        let ctx = tools::tool_context_for_with_read_cache(
                            cfg.as_ref(),
                            cfg.allowed_commands.as_ref(),
                            wd.as_path(),
                            rfc.as_ref().map(|a| a.as_ref()),
                            wcl.as_ref(),
                        );
                        tools::http_fetch::run_direct(&args, &ctx)
                    })
                    .await
                    .unwrap_or_else(|e| format!("工具执行 panic：{}", e)),
                    ParallelToolKind::SyncDefault => tokio::task::spawn_blocking(move || {
                        let ctx = tools::tool_context_for_with_read_cache(
                            cfg.as_ref(),
                            cfg.allowed_commands.as_ref(),
                            wd.as_path(),
                            rfc.as_ref().map(|a| a.as_ref()),
                            wcl.as_ref(),
                        );
                        tools::run_tool(&name, &args, &ctx)
                    })
                    .await
                    .unwrap_or_else(|e| format!("工具执行 panic：{}", e)),
                };
                info!(
                    target: LOG_TARGET,
                    "并行工具完成 tool={} elapsed_ms={}",
                    name_for_log,
                    t_tool.elapsed().as_millis()
                );
                (name_for_return, args_for_return, result)
            };
            match tokio::time::timeout(Duration::from_secs(wall_secs), work).await {
                Ok(triple) => triple,
                Err(_) => {
                    warn!(
                        target: LOG_TARGET,
                        "并行工具墙上时钟超时 tool={} wall_secs={}",
                        name_timeout,
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
    let result_map: HashMap<(&str, &str), &str> = unique_outcomes
        .iter()
        .map(|(n, a, r)| ((n.as_str(), a.as_str()), r.as_str()))
        .collect();

    for tc in tool_calls {
        if abort_tool_batch_if_sse_closed(
            out,
            "SSE sender closed during parallel tool batch, aborting remainder",
        )
        .await
        {
            return ExecuteToolsBatchOutcome::AbortedSse;
        }
        emit_tool_call_summary_sse(out, &tc.function.name, &tc.function.arguments).await;
        let cached = result_map
            .get(&(tc.function.name.as_str(), tc.function.arguments.as_str()))
            .copied()
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
            EmitToolResultParams {
                cfg,
                out,
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

/// 串行路径：`dispatch_tool`、只读结果缓存、写操作后清缓存。
async fn execute_tools_serial(
    ctx: ExecuteToolsCommonCtx<'_>,
    workspace_changed: &mut bool,
) -> ExecuteToolsBatchOutcome {
    let ExecuteToolsCommonCtx {
        tool_calls,
        per_coord,
        messages,
        cfg,
        effective_working_dir,
        workspace_is_set,
        read_file_turn_cache,
        workspace_changelist,
        out,
        echo_terminal_transcript,
        terminal_tool_display_max_chars,
        tool_result_envelope_v1,
        web_tool_ctx,
        cli_tool_ctx,
        mcp_session,
        request_chrome_trace,
        step_executor_constraint,
    } = ctx;

    let mut readonly_cache: HashMap<(String, String), String> = HashMap::new();
    for tc in tool_calls {
        if abort_tool_batch_if_sse_closed(
            out,
            "SSE sender closed during tool execution, aborting remaining tools",
        )
        .await
        {
            return ExecuteToolsBatchOutcome::AbortedSse;
        }

        let name = tc.function.name.clone();
        let args = tc.function.arguments.clone();
        let id = tc.id.clone();
        emit_tool_call_summary_sse(out, &name, &args).await;
        info!(target: LOG_TARGET, "调用工具 tool={}", name);
        debug!(
            target: LOG_TARGET,
            "工具调用参数摘要 tool={} args_preview={}",
            name,
            crate::redact::tool_arguments_preview_for_log(&args)
        );

        if let Some(k) = step_executor_constraint
            && !tool_allowed_for_step_executor_kind(cfg.as_ref(), name.as_str(), k)
        {
            let denied = format!(
                "工具「{}」不在本步子代理角色 {:?} 的允许列表内；请改用该步允许的只读/补丁/测试工具，或让规划器省略 executor_kind。",
                name, k
            );
            warn!(target: LOG_TARGET, "{}", denied);
            let env = ToolEnvelopeContext {
                tool_call_id: id.as_str(),
                execution_mode: "serial",
                parallel_batch_id: None,
            };
            emit_tool_result_sse_and_append(
                messages,
                per_coord,
                EmitToolResultParams {
                    cfg,
                    out,
                    echo_terminal_transcript,
                    terminal_tool_display_max_chars,
                    tool_result_envelope_v1,
                    name: name.as_str(),
                    args: args.as_str(),
                    id: id.as_str(),
                    result: denied,
                    reflection_inject: None,
                    envelope_ctx: Some(env),
                },
            )
            .await;
            continue;
        }

        let is_readonly = tool_registry::is_readonly_tool(cfg.as_ref(), name.as_str());
        let cache_key = (name.clone(), args.clone());

        if is_readonly && let Some(cached) = readonly_cache.get(&cache_key) {
            info!(
                target: LOG_TARGET,
                "工具结果命中缓存（只读去重） tool={}",
                name
            );
            let env = ToolEnvelopeContext {
                tool_call_id: id.as_str(),
                execution_mode: "serial",
                parallel_batch_id: None,
            };
            emit_tool_result_sse_and_append(
                messages,
                per_coord,
                EmitToolResultParams {
                    cfg,
                    out,
                    echo_terminal_transcript,
                    terminal_tool_display_max_chars,
                    tool_result_envelope_v1,
                    name: name.as_str(),
                    args: args.as_str(),
                    id: id.as_str(),
                    result: cached.clone(),
                    reflection_inject: None,
                    envelope_ctx: Some(env),
                },
            )
            .await;
            continue;
        }

        let t_tool = Instant::now();
        let runtime = if let Some(cctx) = cli_tool_ctx {
            ToolRuntime::Cli {
                workspace_changed,
                ctx: cctx,
            }
        } else {
            ToolRuntime::Web {
                workspace_changed,
                ctx: web_tool_ctx,
            }
        };
        let (result, reflection_inject) =
            tool_registry::dispatch_tool(tool_registry::DispatchToolParams {
                runtime,
                per_coord,
                cfg,
                effective_working_dir,
                workspace_is_set,
                name: &name,
                args: &args,
                tc,
                read_file_turn_cache: read_file_turn_cache.clone(),
                workspace_changelist: workspace_changelist.cloned(),
                mcp_session,
                request_chrome_merge: request_chrome_trace.clone(),
            })
            .await;

        info!(
            target: LOG_TARGET,
            "工具调用完成 tool={} elapsed_ms={}",
            name,
            t_tool.elapsed().as_millis()
        );

        if cfg.codebase_semantic_search_enabled
            && cfg.codebase_semantic_invalidate_on_workspace_change
        {
            let cs = crate::codebase_semantic_index::CodebaseSemanticToolParams::from_agent_config(
                cfg.as_ref(),
            );
            if cs.enabled && cs.invalidate_on_workspace_change {
                let should_apply = if is_readonly {
                    *workspace_changed
                } else {
                    crate::codebase_semantic_invalidation::tool_output_semantic_success(
                        name.as_str(),
                        result.as_str(),
                    )
                };
                if should_apply {
                    let inv = crate::codebase_semantic_invalidation::invalidation_for_tool_call(
                        cfg.as_ref(),
                        name.as_str(),
                        args.as_str(),
                    )
                    .or_else(|| {
                        (*workspace_changed).then_some(
                            crate::codebase_semantic_invalidation::CodebaseSemanticInvalidation::FullWorkspace,
                        )
                    });
                    if let Some(inv) = inv {
                        crate::codebase_semantic_invalidation::apply_after_successful_tool(
                            effective_working_dir,
                            cs.index_sqlite_path.as_str(),
                            inv,
                        );
                    }
                }
            }
        }

        if (!is_readonly || *workspace_changed)
            && let Some(c) = read_file_turn_cache.as_ref()
        {
            c.clear();
        }

        if is_readonly {
            readonly_cache.insert(cache_key, result.clone());
        } else {
            readonly_cache.clear();
        }

        let env = ToolEnvelopeContext {
            tool_call_id: id.as_str(),
            execution_mode: "serial",
            parallel_batch_id: None,
        };
        emit_tool_result_sse_and_append(
            messages,
            per_coord,
            EmitToolResultParams {
                cfg,
                out,
                echo_terminal_transcript,
                terminal_tool_display_max_chars,
                tool_result_envelope_v1,
                name: name.as_str(),
                args: args.as_str(),
                id: id.as_str(),
                result,
                reflection_inject,
                envelope_ctx: Some(env),
            },
        )
        .await;
    }

    ExecuteToolsBatchOutcome::Finished
}

async fn per_execute_tools_common(ctx: ExecuteToolsCommonCtx<'_>) -> ExecuteToolsBatchOutcome {
    let out = ctx.out;

    emit_sse_tool_running(out, true, "execute_tools::batch tool_running true").await;

    let workspace_changed =
        if tool_registry::tool_calls_allow_parallel_sync_batch(ctx.cfg.as_ref(), ctx.tool_calls) {
            let outcome = execute_tools_parallel(ctx).await;
            if matches!(outcome, ExecuteToolsBatchOutcome::AbortedSse) {
                return outcome;
            }
            false
        } else {
            let mut workspace_changed = false;
            let outcome = execute_tools_serial(ctx, &mut workspace_changed).await;
            if matches!(outcome, ExecuteToolsBatchOutcome::AbortedSse) {
                return outcome;
            }
            workspace_changed
        };

    if let Some(tx) = out
        && workspace_changed
    {
        let _ = crate::sse::send_string_logged(
            tx,
            encode_message(SsePayload::WorkspaceChanged {
                workspace_changed: true,
            }),
            "execute_tools::batch workspace_changed",
        )
        .await;
    }
    emit_sse_tool_running(out, false, "execute_tools::batch tool_running false").await;

    ExecuteToolsBatchOutcome::Finished
}

/// E：执行一批 tool 调用，写入 tool / 反思 user，并发送 SSE 片段。
pub(crate) async fn per_execute_tools_web(
    tool_calls: &[ToolCall],
    per_coord: &mut PerCoordinator,
    messages: &mut Vec<Message>,
    ctx: WebExecuteCtx<'_>,
) -> ExecuteToolsBatchOutcome {
    let WebExecuteCtx {
        cfg,
        effective_working_dir,
        workspace_is_set,
        read_file_turn_cache,
        out,
        web_tool_ctx,
        cli_tool_ctx,
        echo_terminal_transcript,
        mcp_session,
        workspace_changelist,
        request_chrome_trace,
        step_executor_constraint,
    } = ctx;

    let _tool_trace = request_chrome_trace
        .as_ref()
        .map(|t| t.enter_section("agent.tools_batch"));

    per_execute_tools_common(ExecuteToolsCommonCtx {
        tool_calls,
        per_coord,
        messages,
        cfg,
        effective_working_dir,
        workspace_is_set,
        read_file_turn_cache,
        workspace_changelist,
        out,
        echo_terminal_transcript,
        terminal_tool_display_max_chars: cfg.command_max_output_len,
        tool_result_envelope_v1: cfg.tool_result_envelope_v1,
        web_tool_ctx,
        cli_tool_ctx,
        mcp_session,
        request_chrome_trace,
        step_executor_constraint,
    })
    .await
}
