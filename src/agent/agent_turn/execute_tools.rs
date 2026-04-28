//! E 步：执行 tool_calls（SSE/终端、并行只读批、串行带缓存）。

use std::collections::{HashMap, HashSet, VecDeque};

use crate::agent_role_turn::{tool_allowed_for_turn, turn_tool_denied_message};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures_util::stream::{self, StreamExt};
use log::{info, warn};
use tokio::sync::mpsc;
use tracing::Instrument;

static PARALLEL_READONLY_TOOL_BATCH_SEQ: AtomicU64 = AtomicU64::new(1);

use crate::agent::per_coord::PerCoordinator;
use crate::agent::plan_artifact::PlanStepExecutorKind;
use crate::agent::workflow_tool_dispatch;
use crate::clarification_questionnaire::maybe_emit_clarification_questionnaire_sse;
use crate::config::AgentConfig;
use crate::long_term_memory::LongTermMemoryRuntime;
use crate::sse::{SsePayload, ThinkingTraceBody, ToolCallSummary, ToolResultBody, encode_message};
use crate::tool_registry::{self, HandlerId, ToolRuntime, handler_id_for};
use crate::tool_result::{self, NormalizedToolEnvelope, ToolEnvelopeContext, parse_legacy_output};
use crate::tools;
use crate::types::{Message, Tool, ToolCall, message_content_byte_len_for_estimate};
use crate::workspace_changelist::WorkspaceChangelist;

use super::sub_agent_policy::{
    executor_kind_tool_denied_body, tool_allowed_for_step_executor_kind,
};

/// 本模块 `tracing` / `log` 的 `target`，便于 `RUST_LOG=crabmate::execute_tools` 过滤。
const LOG_TARGET: &str = "crabmate::execute_tools";

fn context_snapshot_for_trace(messages: &[Message]) -> String {
    const MAX: usize = 600;
    let n = messages.len();
    let parts: Vec<String> = messages
        .iter()
        .rev()
        .take(6)
        .rev()
        .map(|m| {
            let role = m.role.as_str();
            let mut c = message_content_byte_len_for_estimate(&m.content);
            if let Some(ref r) = m.reasoning_content {
                c = c.saturating_add(r.len());
            }
            format!("{role}:~{c}b")
        })
        .collect();
    let mut s = format!("messages={n} [{}]", parts.join(", "));
    if s.len() > MAX {
        s.truncate(MAX);
        s.push('…');
    }
    s
}

fn parse_run_command_payload(args_json: &str) -> Option<(String, Vec<String>)> {
    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
    let command = v.get("command")?.as_str()?.trim().to_string();
    let args = v
        .get("args")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some((command, args))
}

fn classify_run_command_failure_family_from_invocation(
    command: &str,
    args: &[String],
) -> Option<&'static str> {
    if command == "cd" {
        return Some("shell_builtin_cd_unavailable");
    }
    if args.iter().any(|a| a.contains("..") || a.starts_with('/')) {
        return Some("path_parent_or_absolute_forbidden");
    }
    None
}

fn classify_run_command_failure_family_from_result(result: &str) -> Option<&'static str> {
    if result.contains("参数不允许包含 \"..\" 或绝对路径（以 / 开头）") {
        return Some("path_parent_or_absolute_forbidden");
    }
    if result.contains("命令 \"cd\" 不存在或在当前环境中不可用") {
        return Some("shell_builtin_cd_unavailable");
    }
    if result.contains("当前目录缺少 Cargo.toml") {
        return Some("cargo_manifest_missing");
    }
    None
}

fn cargo_subcommand_needs_manifest(args: &[String]) -> bool {
    let Some(sub) = args.iter().find(|s| !s.starts_with('-')) else {
        return false;
    };
    matches!(
        sub.as_str(),
        "build" | "run" | "test" | "check" | "clippy" | "fmt"
    )
}

fn find_cargo_toml_candidates(base: &Path, max_depth: usize, max_hits: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut q: VecDeque<(std::path::PathBuf, usize)> = VecDeque::new();
    q.push_back((base.to_path_buf(), 0));
    while let Some((dir, depth)) = q.pop_front() {
        if out.len() >= max_hits {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for ent in entries.flatten() {
            let path = ent.path();
            if path.is_file() && path.file_name().is_some_and(|n| n == "Cargo.toml") {
                if let Ok(rel) = path.strip_prefix(base) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
                if out.len() >= max_hits {
                    break;
                }
            } else if path.is_dir() && depth < max_depth {
                q.push_back((path, depth + 1));
            }
        }
    }
    out
}

fn run_command_cargo_workdir_preflight_error(
    tool_name: &str,
    tool_args_json: &str,
    effective_working_dir: &Path,
) -> Option<String> {
    if tool_name != "run_command" {
        return None;
    }
    let (command, args) = parse_run_command_payload(tool_args_json)?;
    if command != "cargo" {
        return None;
    }
    if args.iter().any(|a| a == "--manifest-path") {
        return None;
    }
    if !cargo_subcommand_needs_manifest(&args) {
        return None;
    }
    if effective_working_dir.join("Cargo.toml").is_file() {
        return None;
    }

    let candidates = find_cargo_toml_candidates(effective_working_dir, 3, 3);
    let command_preview = format!("cargo {}", args.join(" "));
    if candidates.len() == 1 {
        return Some(format!(
            "错误：当前目录缺少 Cargo.toml，已阻止重复无效执行。请改为：`{command_preview} --manifest-path {}`",
            candidates[0]
        ));
    }
    if candidates.len() > 1 {
        return Some(format!(
            "错误：当前目录缺少 Cargo.toml，且发现多个候选（{}）。请显式使用 `--manifest-path <path>` 后重试。",
            candidates.join(", ")
        ));
    }
    Some(
        "错误：当前目录缺少 Cargo.toml，已阻止重复无效执行。请先定位项目根目录，或改用 `--manifest-path <path>`。"
            .to_string(),
    )
}

async fn emit_thinking_trace_sse(
    out: Option<&mpsc::Sender<String>>,
    cfg: &AgentConfig,
    body: ThinkingTraceBody,
) {
    if !cfg.agent_thinking_trace_enabled {
        return;
    }
    let Some(tx) = out else {
        return;
    };
    let _ = crate::sse::send_string_logged(
        tx,
        encode_message(SsePayload::ThinkingTrace { trace: body }),
        "execute_tools::thinking_trace",
    )
    .await;
}

fn trace_parallel_tool_child_span(
    tracing_turn: Option<&Arc<crate::observability::TracingChatTurn>>,
    tool_call_id: &str,
) -> tracing::Span {
    match tracing_turn {
        Some(t) => {
            t.record_tool_call_id_for_log(tool_call_id);
            let id_short = crate::redact::preview_chars(
                tool_call_id.trim(),
                crate::observability::CHAT_TURN_TOOL_CALL_ID_FIELD_MAX_CHARS,
            );
            tracing::span!(
                parent: t.span.id(),
                tracing::Level::INFO,
                "parallel_tool",
                tool_call_id = %id_short,
            )
        }
        None => tracing::Span::none(),
    }
}

/// 并行执行时工具的分类，用于在构建 fut 前预分类，消除 if/else if/else 字符串比较。
#[derive(Clone, Copy)]
enum ParallelToolKind {
    HttpFetch,
    GetWeather,
    WebSearch,
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
    /// 本会话完整工具定义（未按步收窄）；用于子代理越权提示中的允许工具名列表。
    pub tools_defs_full: &'a [Tool],
    /// 多角色工具白名单；`None` 不限制。
    pub turn_allow: Option<&'a HashSet<String>>,
    pub long_term_memory: Option<Arc<LongTermMemoryRuntime>>,
    pub long_term_memory_scope_id: Option<String>,
    pub tracing_chat_turn: Option<Arc<crate::observability::TracingChatTurn>>,
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
                goal_id: None,
                result_version: norm.envelope_version,
                summary: tool_summary,
                output: result.to_string(),
                ok: Some(norm.ok),
                exit_code: norm.exit_code,
                error_code: norm.error_code.clone(),
                failure_category: norm.failure_category.clone(),
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

async fn emit_timeline_log_sse(
    out: Option<&mpsc::Sender<String>>,
    kind: &str,
    title: String,
    detail: Option<String>,
    log_label: &'static str,
) {
    crate::turn_replay_dump::append_turn_replay_event_if_configured(
        kind,
        title.as_str(),
        detail.as_deref(),
    );
    let Some(tx) = out else {
        return;
    };
    let _ = crate::sse::send_string_logged(
        tx,
        encode_message(SsePayload::TimelineLog {
            log: crate::sse::protocol::TimelineLogBody {
                kind: kind.to_string(),
                title,
                detail,
            },
        }),
        log_label,
    )
    .await;
}

async fn emit_tool_result_sse_and_append(
    messages: &mut Vec<Message>,
    per_coord: &mut PerCoordinator,
    p: EmitToolResultParams<'_>,
) {
    let tool_t0 = std::time::Instant::now();
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
    let parsed_for_timeline = parse_legacy_output(name, result.as_str());

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
        maybe_emit_clarification_questionnaire_sse(Some(tx), name, args, result.as_str()).await;
    }

    let status = if parsed_for_timeline.ok {
        "ok"
    } else {
        "failed"
    };
    let detail = tool_summary.as_ref().map(|s| {
        format!(
            "status={status}, summary={s}, exit_code={}",
            parsed_for_timeline
                .exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "-".to_string())
        )
    });
    emit_timeline_log_sse(
        out,
        "tool_step_finished",
        name.to_string(),
        detail,
        "execute_tools::timeline tool_step_finished",
    )
    .await;
    crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
        "tool_call_finished",
        name,
        Some(&serde_json::json!({
            "tool_call_id": id,
            "tool_name": name,
            "execution_mode": envelope_ctx.map(|e| e.execution_mode),
            "parallel_batch_id": envelope_ctx.and_then(|e| e.parallel_batch_id),
            "ok": parsed_for_timeline.ok,
            "exit_code": parsed_for_timeline.exit_code,
            "error_code": parsed_for_timeline.error_code,
            "failure_category": parsed_for_timeline
                .error_code
                .as_deref()
                .map(|c| crate::tool_result::failure_category_for_error_code(c).as_str().to_string()),
            "retryable": crate::tool_result::tool_error_retryable_heuristic(
                parsed_for_timeline.error_code.as_deref()
            ),
            "summary": tool_summary,
            "stdout_preview": crate::redact::preview_chars(&parsed_for_timeline.stdout, 1200),
            "stdout_preview_truncated": parsed_for_timeline.stdout.chars().count() > 1200,
            "stderr_preview": crate::redact::preview_chars(&parsed_for_timeline.stderr, 1200),
            "stderr_preview_truncated": parsed_for_timeline.stderr.chars().count() > 1200,
            "result_preview": crate::redact::single_line_preview(result.as_str(), 1200),
            "result_preview_truncated": result.chars().count() > 1200,
            "tool_elapsed_ms": tool_t0.elapsed().as_millis(),
            "phase": "tool_execution",
        })),
    );

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

    emit_thinking_trace_sse(
        out,
        cfg.as_ref(),
        ThinkingTraceBody {
            op: "tool_done".into(),
            node_id: Some(format!("tool:{name}")),
            parent_id: None,
            title: Some(name.to_string()),
            chunk: None,
            context_snapshot: Some(context_snapshot_for_trace(messages)),
        },
    )
    .await;
}

/// SSE 发送端已关闭（与外层 `run_agent_turn` 早退判断一致）。
pub(crate) fn sse_sender_closed(out: Option<&mpsc::Sender<String>>) -> bool {
    out.is_some_and(|tx| tx.is_closed())
}

async fn emit_tool_call_summary_sse(
    out: Option<&mpsc::Sender<String>>,
    cfg: &AgentConfig,
    tool_call_id: &str,
    name: &str,
    args: &str,
    messages: &[Message],
) {
    let args_preview = crate::redact::tool_arguments_preview_for_sse(args);
    let Some(tx) = out else {
        crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
            "tool_call_started",
            name,
            Some(&serde_json::json!({
                "tool_call_id": tool_call_id,
                "tool_name": name,
                "args_preview": args_preview,
                "phase": "tool_execution",
            })),
        );
        return;
    };
    let args_parsed: Option<serde_json::Value> = serde_json::from_str(args).ok();
    let summary = if let Some(ref parsed) = args_parsed {
        tools::summarize_tool_call_parsed(name, parsed)
    } else {
        tools::summarize_tool_call(name, args)
    }
    .unwrap_or_else(|| format!("tool: {name}"));
    let arguments_preview = Some(args_preview.clone());
    let arguments = cfg
        .sse_tool_call_include_arguments
        .then(|| crate::redact::tool_arguments_redacted_for_sse(args));

    // 记录工具调用参数（脱敏后）
    let args_for_log = crate::redact::tool_arguments_preview_for_log(args);
    info!(
        target: "crabmate::tool_call",
        "[tool_call] name={} args={}",
        name,
        args_for_log
    );
    crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
        "tool_call_started",
        name,
        Some(&serde_json::json!({
            "tool_call_id": tool_call_id,
            "tool_name": name,
            "summary": summary,
            "args_preview": args_preview,
            "args_preview_truncated": args.chars().count() > 1200,
            "phase": "tool_execution",
        })),
    );

    let _ = crate::sse::send_string_logged(
        tx,
        encode_message(SsePayload::ToolCall {
            tool_call: ToolCallSummary {
                name: name.to_string(),
                summary,
                goal_id: None,
                tool_call_id: Some(tool_call_id.to_string()),
                arguments_preview,
                arguments,
            },
        }),
        "execute_tools::tool_call summary",
    )
    .await;
    emit_thinking_trace_sse(
        Some(tx),
        cfg,
        ThinkingTraceBody {
            op: "tool_call".into(),
            node_id: Some(format!("tool:{name}")),
            parent_id: None,
            title: Some(name.to_string()),
            chunk: None,
            context_snapshot: Some(context_snapshot_for_trace(messages)),
        },
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
    tools_defs_full: &'a [Tool],
    turn_allow: Option<&'a HashSet<String>>,
    long_term_memory: Option<Arc<LongTermMemoryRuntime>>,
    long_term_memory_scope_id: Option<String>,
    tracing_chat_turn: Option<Arc<crate::observability::TracingChatTurn>>,
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
        tools_defs_full,
        turn_allow,
        long_term_memory,
        long_term_memory_scope_id,
        tracing_chat_turn,
    } = ctx;

    let tools_defs_hint = Arc::new(tools_defs_full.to_vec());

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
    let parallel_batch_id_ref = parallel_batch_id.as_str();

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
        )
        .await,
    );

    let mut seen_keys: HashSet<(String, String)> = HashSet::with_capacity(tool_calls.len());
    let mut unique_futs = Vec::new();
    for tc in tool_calls {
        let key = (tc.function.name.clone(), tc.function.arguments.clone());
        if !seen_keys.insert(key.clone()) {
            continue;
        }
        let prefetch_err = prefetch_failures.get(&key).cloned();
        let cfg = Arc::clone(cfg);
        let tools_defs_hint = Arc::clone(&tools_defs_hint);
        let wd = effective_working_dir.to_path_buf();
        let rfc = read_file_turn_cache.clone();
        let wcl = workspace_changelist.cloned();
        let ltm = long_term_memory.clone();
        let ltm_scope = long_term_memory_scope_id.clone();
        let name = tc.function.name.clone();
        let args = tc.function.arguments.clone();
        let tc_owned = tc.clone();
        let tool_call_id_for_trace = tc.id.clone();
        let tracing_turn_parallel = tracing_chat_turn.clone();
        let kind = match handler_id_for(name.as_str()) {
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
                    target: LOG_TARGET,
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
                                crate::long_term_memory::tool_context_memory_extras(
                                    cfg.as_ref(),
                                    ltm.clone(),
                                    ltm_scope.as_deref(),
                                );
                            let ctx = tools::tool_context_for_with_read_cache_and_memory(
                                cfg.as_ref(),
                                cfg.allowed_commands.as_ref(),
                                wd.as_path(),
                                rfc.as_ref().map(|a| a.as_ref()),
                                wcl.as_ref(),
                                mem_rt,
                                mem_scope,
                            );
                            tools::http_fetch::run_direct(&args, &ctx)
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
                            tc: &tc_owned,
                            read_file_turn_cache: rfc.clone(),
                            workspace_changelist: wcl.clone(),
                            mcp_session: None,
                            turn_allow,
                            long_term_memory: ltm.clone(),
                            long_term_memory_scope_id: ltm_scope.clone(),
                        })
                        .await
                        .0
                    }
                };
                info!(
                    target: LOG_TARGET,
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
                        target: LOG_TARGET,
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
        emit_tool_call_summary_sse(
            out,
            cfg.as_ref(),
            tc.id.as_str(),
            &tc.function.name,
            &tc.function.arguments,
            messages,
        )
        .await;
        emit_timeline_log_sse(
            out,
            "tool_step_started",
            tc.function.name.clone(),
            Some(format!(
                "args={}",
                crate::redact::tool_arguments_preview_for_sse(&tc.function.arguments)
            )),
            "execute_tools::timeline tool_step_started",
        )
        .await;
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
        tools_defs_full,
        turn_allow,
        long_term_memory,
        long_term_memory_scope_id,
        tracing_chat_turn,
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
        if let Some(ref t) = tracing_chat_turn {
            t.record_tool_call_id_for_log(id.as_str());
        }
        emit_tool_call_summary_sse(out, cfg.as_ref(), id.as_str(), &name, &args, messages).await;
        emit_timeline_log_sse(
            out,
            "tool_step_started",
            name.clone(),
            Some(format!(
                "args={}",
                crate::redact::tool_arguments_preview_for_sse(&args)
            )),
            "execute_tools::timeline tool_step_started",
        )
        .await;
        info!(
            target: LOG_TARGET,
            "调用工具 tool={} args_preview={}",
            name,
            crate::redact::tool_arguments_preview_for_log(&args)
        );

        if let Some(preflight_error) = run_command_cargo_workdir_preflight_error(
            name.as_str(),
            args.as_str(),
            effective_working_dir,
        ) {
            per_coord.mark_tool_failure_signature(
                name.as_str(),
                args.as_str(),
                "cargo_manifest_missing".to_string(),
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
                    result: preflight_error,
                    reflection_inject: None,
                    envelope_ctx: Some(env),
                },
            )
            .await;
            continue;
        }

        if let Some(k) = step_executor_constraint
            && !tool_allowed_for_step_executor_kind(cfg.as_ref(), name.as_str(), k)
        {
            let denied =
                executor_kind_tool_denied_body(cfg.as_ref(), tools_defs_full, name.as_str(), k);
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

        if !tool_allowed_for_turn(name.as_str(), turn_allow) {
            let denied = turn_tool_denied_message(name.as_str());
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

        // 同回合短路：同一 run_command（同 args）曾失败过，则不再原样重试，强制模型切策略。
        if name == "run_command"
            && let Some(prev_error) =
                per_coord.repeated_tool_failure_error_marker(name.as_str(), args.as_str())
        {
            let short_circuit = format!(
                "错误：检测到同命令重复失败，已短路本次调用（error={prev_error}）。请切换策略（例如调整工作目录、改用 --manifest-path、或先做目录/文件探测）。"
            );
            warn!(
                target: LOG_TARGET,
                "run_command 重复失败短路 args_preview={} prev_error={}",
                crate::redact::tool_arguments_preview_for_log(&args),
                prev_error
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
                    result: short_circuit,
                    reflection_inject: None,
                    envelope_ctx: Some(env),
                },
            )
            .await;
            continue;
        }
        if name == "run_command"
            && let Some((command, command_args)) = parse_run_command_payload(args.as_str())
            && let Some(family) = classify_run_command_failure_family_from_invocation(
                command.as_str(),
                command_args.as_slice(),
            )
            && let Some(prev_error) =
                per_coord.repeated_tool_failure_family_marker(name.as_str(), family)
        {
            let short_circuit = format!(
                "错误：检测到同类失败已发生（family={family}, prev_error={prev_error}），已短路本次调用。请直接切换策略，避免继续同类试探。"
            );
            warn!(
                target: LOG_TARGET,
                "run_command 同类失败短路 family={} args_preview={} prev_error={}",
                family,
                crate::redact::tool_arguments_preview_for_log(&args),
                prev_error
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
                    result: short_circuit,
                    reflection_inject: None,
                    envelope_ctx: Some(env),
                },
            )
            .await;
            continue;
        }

        if is_readonly && let Some(cached) = readonly_cache.get(&cache_key) {
            info!(
                target: LOG_TARGET,
                "工具结果命中缓存（只读去重） tool={} args_preview={}",
                name,
                crate::redact::tool_arguments_preview_for_log(&args)
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
        let (result, reflection_inject) = if handler_id_for(name.as_str()) == HandlerId::Workflow {
            workflow_tool_dispatch::dispatch_workflow_execute_tool(
                runtime,
                per_coord,
                cfg,
                effective_working_dir,
                workspace_is_set,
                args.as_str(),
                request_chrome_trace.clone(),
            )
            .await
        } else {
            tool_registry::dispatch_tool(tool_registry::DispatchToolParams {
                runtime,
                cfg,
                effective_working_dir,
                workspace_is_set,
                name: &name,
                args: &args,
                tc,
                read_file_turn_cache: read_file_turn_cache.clone(),
                workspace_changelist: workspace_changelist.cloned(),
                mcp_session,
                turn_allow,
                long_term_memory: long_term_memory.clone(),
                long_term_memory_scope_id: long_term_memory_scope_id.clone(),
            })
            .await
        };

        info!(
            target: LOG_TARGET,
            "工具调用完成 tool={} args_preview={} elapsed_ms={}",
            name,
            crate::redact::tool_arguments_preview_for_log(&args),
            t_tool.elapsed().as_millis()
        );

        if name == "run_command" {
            let parsed = parse_legacy_output(name.as_str(), result.as_str());
            if parsed.ok {
                per_coord.clear_tool_failure_signature(name.as_str(), args.as_str());
                per_coord.clear_tool_failure_families_for_tool(name.as_str());
            } else {
                let marker = parsed.error_code.unwrap_or_else(|| {
                    parsed
                        .exit_code
                        .map(|c| format!("exit_code:{c}"))
                        .unwrap_or_else(|| "unknown".to_string())
                });
                per_coord.mark_tool_failure_signature(name.as_str(), args.as_str(), marker.clone());
                if let Some(family) =
                    classify_run_command_failure_family_from_result(result.as_str())
                {
                    per_coord.mark_tool_failure_family(name.as_str(), family, marker);
                }
            }
        }

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
    let force_serial = std::env::var("CM_REPLAY_FORCE_SERIAL")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"));

    emit_sse_tool_running(out, true, "execute_tools::batch tool_running true").await;

    let workspace_changed = if !force_serial
        && ctx.workspace_is_set
        && crate::agent_role_turn::tool_calls_allow_parallel_for_role(
            ctx.cfg.as_ref(),
            ctx.tool_calls,
            ctx.turn_allow,
        ) {
        crate::turn_replay_dump::append_decision_point_event_if_configured(
            "tool_execution",
            "tool_batch_execution_mode",
            "parallel_readonly_batch",
            "当前批次满足只读并行条件，采用并行只读批执行以提升吞吐",
            serde_json::json!({
                "force_serial": force_serial,
                "workspace_is_set": ctx.workspace_is_set,
                "tool_call_count": ctx.tool_calls.len(),
            }),
            "current_tool_batch",
            None,
        );
        let outcome = execute_tools_parallel(ctx).await;
        if matches!(outcome, ExecuteToolsBatchOutcome::AbortedSse) {
            return outcome;
        }
        false
    } else {
        crate::turn_replay_dump::append_decision_point_event_if_configured(
            "tool_execution",
            "tool_batch_execution_mode",
            "serial",
            if force_serial {
                "环境变量强制串行执行，关闭并行只读批"
            } else {
                "当前批次不满足并行条件，回退串行执行"
            },
            serde_json::json!({
                "force_serial": force_serial,
                "workspace_is_set": ctx.workspace_is_set,
                "tool_call_count": ctx.tool_calls.len(),
            }),
            "current_tool_batch",
            None,
        );
        if force_serial {
            log::info!(
                target: LOG_TARGET,
                "CM_REPLAY_FORCE_SERIAL enabled: force serial tool execution"
            );
            crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
                "tool_batch_mode",
                "force_serial",
                Some(&serde_json::json!({
                    "source": "CM_REPLAY_FORCE_SERIAL",
                    "parallel_disabled": true
                })),
            );
        }
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
        tools_defs_full,
        turn_allow,
        long_term_memory,
        long_term_memory_scope_id,
        tracing_chat_turn,
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
        tools_defs_full,
        turn_allow,
        long_term_memory,
        long_term_memory_scope_id,
        tracing_chat_turn,
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::{
        classify_run_command_failure_family_from_invocation,
        classify_run_command_failure_family_from_result,
    };

    #[test]
    fn classify_family_from_invocation_forbidden_path_and_cd() {
        let cd_args = vec!["tmp".to_string()];
        assert_eq!(
            classify_run_command_failure_family_from_invocation("cd", cd_args.as_slice()),
            Some("shell_builtin_cd_unavailable")
        );

        let bad_args = vec![
            "-c".to_string(),
            "cd build && ../configure Linux_Serial".to_string(),
        ];
        assert_eq!(
            classify_run_command_failure_family_from_invocation("sh", bad_args.as_slice()),
            Some("path_parent_or_absolute_forbidden")
        );
    }

    #[test]
    fn classify_family_from_result_known_failures() {
        assert_eq!(
            classify_run_command_failure_family_from_result(
                "错误：参数不允许包含 \"..\" 或绝对路径（以 / 开头）"
            ),
            Some("path_parent_or_absolute_forbidden")
        );
        assert_eq!(
            classify_run_command_failure_family_from_result(
                "错误：命令 \"cd\" 不存在或在当前环境中不可用（工作目录：/tmp）"
            ),
            Some("shell_builtin_cd_unavailable")
        );
        assert_eq!(
            classify_run_command_failure_family_from_result(
                "错误：当前目录缺少 Cargo.toml，已阻止重复无效执行。"
            ),
            Some("cargo_manifest_missing")
        );
    }
}
