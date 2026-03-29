//! E 步：执行 tool_calls（SSE/终端、并行只读批、串行带缓存）。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use futures_util::stream::{self, StreamExt};
use log::{debug, info};
use tokio::sync::mpsc;

static PARALLEL_READONLY_TOOL_BATCH_SEQ: AtomicU64 = AtomicU64::new(1);

use crate::agent::per_coord::PerCoordinator;
use crate::config::AgentConfig;
use crate::sse::{SsePayload, ToolCallSummary, ToolResultBody, encode_message};
use crate::tool_registry::{self, ToolRuntime};
use crate::tool_result::{
    self, ToolEnvelopeContext, parse_legacy_output, tool_error_retryable_heuristic,
};
use crate::tools;
use crate::types::{Message, ToolCall};

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
}

pub(crate) enum ExecuteToolsBatchOutcome {
    /// 本批工具跑完，继续外层循环
    Finished,
    /// SSE 在工具执行中断开
    AbortedSse,
}

/// 单工具：SSE / 终端回显 + 追加 `tool` 与可选反思 `user`（与串行路径一致）的入参。
struct EmitToolResultParams<'a> {
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

async fn emit_tool_result_sse_and_append(
    messages: &mut Vec<Message>,
    per_coord: &mut PerCoordinator,
    p: EmitToolResultParams<'_>,
) {
    let EmitToolResultParams {
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

    if echo_terminal_transcript {
        let omit_body = matches!(name, "read_file" | "read_dir" | "list_tree");
        let _ = crate::runtime::terminal_cli_transcript::print_tool_result_terminal(
            name,
            args,
            tool_summary.as_deref(),
            &result,
            terminal_tool_display_max_chars,
            omit_body,
        );
    }

    if let Some(tx) = out {
        let parsed = parse_legacy_output(name, &result);
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
        let retryable = if parsed.ok {
            None
        } else {
            Some(tool_error_retryable_heuristic(parsed.error_code.as_deref()))
        };
        let tool_call_id = envelope_ctx.map(|c| c.tool_call_id.to_string());
        let execution_mode = envelope_ctx.map(|c| c.execution_mode.to_string());
        let parallel_batch_id = envelope_ctx
            .and_then(|c| c.parallel_batch_id)
            .map(|s| s.to_string());
        let _ = crate::sse::send_string_logged(
            tx,
            encode_message(SsePayload::ToolResult {
                tool_result: ToolResultBody {
                    name: name.to_string(),
                    summary: tool_summary.clone(),
                    output: result.clone(),
                    ok: Some(parsed.ok),
                    exit_code: parsed.exit_code,
                    error_code: parsed.error_code.clone(),
                    retryable,
                    tool_call_id,
                    execution_mode,
                    parallel_batch_id,
                    stdout,
                    stderr,
                },
            }),
            "execute_tools::emit_tool_result_sse",
        )
        .await;
    }

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
    info!(target: "crabmate", "{reason}");
    if let Some(tx) = out {
        let _ = crate::sse::send_string_logged(
            tx,
            encode_message(SsePayload::ToolRunning {
                tool_running: false,
            }),
            "execute_tools::abort_tool_batch tool_running false",
        )
        .await;
    }
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
    out: Option<&'a mpsc::Sender<String>>,
    echo_terminal_transcript: bool,
    terminal_tool_display_max_chars: usize,
    tool_result_envelope_v1: bool,
    web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    cli_tool_ctx: Option<&'a tool_registry::CliToolRuntime>,
    mcp_session: Option<&'a std::sync::Arc<tokio::sync::Mutex<crate::mcp::McpClientSession>>>,
}

async fn per_execute_tools_common(ctx: ExecuteToolsCommonCtx<'_>) -> ExecuteToolsBatchOutcome {
    let ExecuteToolsCommonCtx {
        tool_calls,
        per_coord,
        messages,
        cfg,
        effective_working_dir,
        workspace_is_set,
        read_file_turn_cache,
        out,
        echo_terminal_transcript,
        terminal_tool_display_max_chars,
        tool_result_envelope_v1,
        web_tool_ctx,
        cli_tool_ctx,
        mcp_session,
    } = ctx;
    let mut workspace_changed = false;

    if let Some(tx) = out {
        let _ = crate::sse::send_string_logged(
            tx,
            encode_message(SsePayload::ToolRunning { tool_running: true }),
            "execute_tools::batch tool_running true",
        )
        .await;
    }

    if tool_registry::tool_calls_allow_parallel_sync_batch(tool_calls) {
        let dedup_count = dedup_readonly_tool_calls_count(tool_calls);
        let parallel_max = cfg.parallel_readonly_tools_max.max(1);
        info!(
            target: "crabmate",
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
            let name = tc.function.name.clone();
            let args = tc.function.arguments.clone();
            unique_futs.push(async move {
                if let Some(err) = prefetch_err {
                    return (name, args, err);
                }
                info!(target: "crabmate", "并行工具开始 tool={}", name);
                debug!(
                    target: "crabmate",
                    "工具调用参数摘要 tool={} args_preview={}",
                    name,
                    crate::redact::tool_arguments_preview_for_log(&args)
                );
                let t_tool = Instant::now();
                let tool_name = name.clone();
                let tool_args = args.clone();
                let result = if name == "http_fetch" {
                    tokio::task::spawn_blocking(move || {
                        let ctx = tools::tool_context_for_with_read_cache(
                            cfg.as_ref(),
                            cfg.allowed_commands.as_ref(),
                            wd.as_path(),
                            rfc.as_ref().map(|a| a.as_ref()),
                        );
                        tools::http_fetch::run_direct(&tool_args, &ctx)
                    })
                    .await
                    .unwrap_or_else(|e| format!("工具执行 panic：{}", e))
                } else if crate::tool_registry::sync_default_runs_inline(&name) {
                    let ctx = tools::tool_context_for_with_read_cache(
                        cfg.as_ref(),
                        cfg.allowed_commands.as_ref(),
                        wd.as_path(),
                        rfc.as_ref().map(|a| a.as_ref()),
                    );
                    tools::run_tool(&tool_name, &tool_args, &ctx)
                } else {
                    tokio::task::spawn_blocking(move || {
                        let ctx = tools::tool_context_for_with_read_cache(
                            cfg.as_ref(),
                            cfg.allowed_commands.as_ref(),
                            wd.as_path(),
                            rfc.as_ref().map(|a| a.as_ref()),
                        );
                        tools::run_tool(&tool_name, &tool_args, &ctx)
                    })
                    .await
                    .unwrap_or_else(|e| format!("工具执行 panic：{}", e))
                };
                info!(
                    target: "crabmate",
                    "并行工具完成 tool={} elapsed_ms={}",
                    name,
                    t_tool.elapsed().as_millis()
                );
                (name, args, result)
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
    } else {
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
            info!(target: "crabmate", "调用工具 tool={}", name);
            debug!(
                target: "crabmate",
                "工具调用参数摘要 tool={} args_preview={}",
                name,
                crate::redact::tool_arguments_preview_for_log(&args)
            );

            let is_readonly = tool_registry::is_readonly_tool(&name);
            let cache_key = (name.clone(), args.clone());

            if is_readonly && let Some(cached) = readonly_cache.get(&cache_key) {
                info!(
                    target: "crabmate",
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
                    workspace_changed: &mut workspace_changed,
                    ctx: cctx,
                }
            } else {
                ToolRuntime::Web {
                    workspace_changed: &mut workspace_changed,
                    ctx: web_tool_ctx,
                }
            };
            let (result, reflection_inject) = tool_registry::dispatch_tool(
                runtime,
                per_coord,
                cfg,
                effective_working_dir,
                workspace_is_set,
                &name,
                &args,
                tc,
                read_file_turn_cache.clone(),
                mcp_session,
            )
            .await;

            info!(
                target: "crabmate",
                "工具调用完成 tool={} elapsed_ms={}",
                name,
                t_tool.elapsed().as_millis()
            );

            if (!is_readonly || workspace_changed)
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
    }

    if let Some(tx) = out {
        if workspace_changed {
            let _ = crate::sse::send_string_logged(
                tx,
                encode_message(SsePayload::WorkspaceChanged {
                    workspace_changed: true,
                }),
                "execute_tools::batch workspace_changed",
            )
            .await;
        }
        let _ = crate::sse::send_string_logged(
            tx,
            encode_message(SsePayload::ToolRunning {
                tool_running: false,
            }),
            "execute_tools::batch tool_running false",
        )
        .await;
    }

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
    } = ctx;

    per_execute_tools_common(ExecuteToolsCommonCtx {
        tool_calls,
        per_coord,
        messages,
        cfg,
        effective_working_dir,
        workspace_is_set,
        read_file_turn_cache,
        out,
        echo_terminal_transcript,
        terminal_tool_display_max_chars: cfg.command_max_output_len,
        tool_result_envelope_v1: cfg.tool_result_envelope_v1,
        web_tool_ctx,
        cli_tool_ctx,
        mcp_session,
    })
    .await
}
