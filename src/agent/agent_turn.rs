//! 单轮 Agent 循环的步骤拆分：与「规划–执行–反思」命名对齐的调用边界（P/E/R）。
//!
//! **命名说明**：此处的 **P（Plan）** 指「向模型要本轮输出」——即一次 `llm::complete_chat_retrying`（内部 `llm::api::stream_chat`），由模型产出正文或 `tool_calls`，
//! **不是**独立的符号规划器。**E** 为执行工具；**R** 为终答阶段是否满足结构化规划等（见 `per_coord::after_final_assistant`）。
//!
//! 被 crate 根 [`crate::run_agent_turn`]（Web/CLI）与 Axum handler 共用。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::config::{AgentConfig, PlannerExecutorMode};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::future::join_all;
use log::{debug, info};
use tokio::sync::mpsc;

use super::per_coord::PerCoordinator;
use crate::llm::{complete_chat_retrying, no_tools_chat_request, tool_chat_request};
use crate::sse::{
    SseErrorBody, SsePayload, StagedPlanFinishedBody, StagedPlanStartedBody,
    StagedPlanStepFinishedBody, StagedPlanStepStartedBody, ToolResultBody, encode_message,
};
use crate::tool_registry::{self, ToolRuntime};
use crate::tool_result;
use crate::tools;
use crate::types::{
    LlmSeedOverride, Message, ToolCall, USER_CANCELLED_FINISH_REASON, is_chat_ui_separator,
};

static STAGED_PLAN_SEQ: AtomicU64 = AtomicU64::new(1);

/// Web/CLI 在提交前会追加一条 `content` 为空的 `assistant` 供流式写入。
/// Agent 侧若再 `push` 一条同轮助手，会得到 `[…, 空助手, 真助手]`，与单条气泡不对齐。
/// 对**末尾**且仍为空、无 `tool_calls` 的助手占位则**就地替换**。
fn push_assistant_merging_trailing_empty_placeholder(messages: &mut Vec<Message>, msg: Message) {
    if msg.role != "assistant" {
        messages.push(msg);
        return;
    }
    if let Some(last) = messages.last_mut()
        && last.role == "assistant"
        && last.tool_calls.is_none()
        && last
            .content
            .as_deref()
            .map(|s| s.trim())
            .unwrap_or("")
            .is_empty()
    {
        *last = msg;
        return;
    }
    messages.push(msg);
}

/// 规划轮默认 system 追加（可被 `[agent] staged_plan_phase_instruction` 覆盖）。
fn staged_plan_phase_instruction_default() -> String {
    format!(
        "【分阶段规划模式 · 规划轮】请仅根据用户消息做任务拆解，不要调用任何工具，不要执行命令或读写文件。\n\
         在回复正文中必须用 Markdown 代码围栏（语言标记为 json）给出一个合法 JSON 对象，且满足：\n\
         {}\n\
         可辅以简短自然语言说明；后续系统将按 steps 顺序逐步下发执行指令。",
        super::plan_artifact::PLAN_V1_SCHEMA_RULES
    )
}

fn staged_plan_queue_summary_text(
    plan: &super::plan_artifact::AgentReplyPlanV1,
    completed_count: usize,
) -> String {
    let n = plan.steps.len();
    let steps_md =
        super::plan_artifact::format_plan_steps_markdown_for_staged_queue(plan, completed_count);
    let header = format!(
        "{}共 {} 步",
        crate::runtime::plan_section::STAGED_PLAN_SECTION_HEADER,
        n,
    );
    let body = format!("{}\n\n{}", header, steps_md);
    if n > 0 && completed_count >= n {
        format!("[✓] 全部完成\n\n{}", body)
    } else {
        body
    }
}

async fn emit_chat_ui_separator_sse(out: Option<&mpsc::Sender<String>>, short: bool) {
    if let Some(tx) = out {
        let _ = tx
            .send(encode_message(SsePayload::ChatUiSeparator { short }))
            .await;
    }
}

/// 本轮用户消息之后插入与分步结束相同的短分隔线（不进入模型）；若下一条已是分隔线则跳过。
fn insert_separator_after_last_user_for_turn(messages: &mut Vec<Message>) {
    let Some(user_idx) = messages.iter().rposition(|m| m.role == "user") else {
        return;
    };
    if messages.get(user_idx + 1).is_some_and(is_chat_ui_separator) {
        return;
    }
    let sep = Message::chat_ui_separator(true);
    match messages.get(user_idx + 1) {
        Some(m) if m.role == "assistant" => {
            messages.insert(user_idx + 1, sep);
        }
        _ => {
            messages.push(sep);
        }
    }
}

async fn send_staged_plan_notice(
    out: Option<&mpsc::Sender<String>>,
    echo_terminal: bool,
    clear_before: bool,
    text: impl Into<String>,
) {
    let text = text.into();
    if text.is_empty() {
        return;
    }
    // CLI（`out: None` 且 `render_to_terminal`）无 SSE，把规划/步骤打到 stdout
    if echo_terminal {
        let _ =
            crate::runtime::terminal_cli_transcript::print_staged_plan_notice(clear_before, &text);
    }
    if let Some(tx) = out {
        let _ = tx
            .send(encode_message(SsePayload::StagedPlanNotice {
                text,
                clear_before,
            }))
            .await;
    }
}

fn next_staged_plan_id() -> String {
    let ts_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let seq = STAGED_PLAN_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("staged-{ts_ms}-{seq}")
}

async fn send_staged_plan_started(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    total_steps: usize,
) {
    let Some(tx) = out else {
        return;
    };
    let _ = tx
        .send(encode_message(SsePayload::StagedPlanStarted {
            started: StagedPlanStartedBody {
                plan_id: plan_id.to_string(),
                total_steps,
            },
        }))
        .await;
}

async fn send_staged_plan_step_started(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    step_id: &str,
    step_index: usize,
    total_steps: usize,
    description: &str,
) {
    let Some(tx) = out else {
        return;
    };
    let _ = tx
        .send(encode_message(SsePayload::StagedPlanStepStarted {
            started: StagedPlanStepStartedBody {
                plan_id: plan_id.to_string(),
                step_id: step_id.to_string(),
                step_index,
                total_steps,
                description: description.to_string(),
            },
        }))
        .await;
}

async fn send_staged_plan_step_finished(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    step_id: &str,
    step_index: usize,
    total_steps: usize,
    status: &str,
) {
    let Some(tx) = out else {
        return;
    };
    let _ = tx
        .send(encode_message(SsePayload::StagedPlanStepFinished {
            finished: StagedPlanStepFinishedBody {
                plan_id: plan_id.to_string(),
                step_id: step_id.to_string(),
                step_index,
                total_steps,
                status: status.to_string(),
            },
        }))
        .await;
}

async fn send_staged_plan_finished(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    total_steps: usize,
    completed_steps: usize,
    status: &str,
) {
    let Some(tx) = out else {
        return;
    };
    let _ = tx
        .send(encode_message(SsePayload::StagedPlanFinished {
            finished: StagedPlanFinishedBody {
                plan_id: plan_id.to_string(),
                total_steps,
                completed_steps,
                status: status.to_string(),
            },
        }))
        .await;
}

// --- P：向模型要本轮输出（含重试）---

/// P：构造请求并调用模型（`no_stream` 为 true 时走 `stream: false`），**不**修改 `messages`。
pub(crate) struct PerPlanCallModelParams<'a> {
    pub llm_backend: &'a (dyn crate::llm::ChatCompletionsBackend + 'static),
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub cfg: &'a AgentConfig,
    pub tools_defs: &'a [crate::types::Tool],
    pub messages: &'a [Message],
    pub out: Option<&'a mpsc::Sender<String>>,
    pub render_to_terminal: bool,
    pub no_stream: bool,
    pub cancel: Option<&'a AtomicBool>,
    pub plain_terminal_stream: bool,
    pub temperature_override: Option<f32>,
    pub seed_override: LlmSeedOverride,
}

pub(crate) async fn per_plan_call_model_retrying(
    p: PerPlanCallModelParams<'_>,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let PerPlanCallModelParams {
        llm_backend,
        client,
        api_key,
        cfg,
        tools_defs,
        messages,
        out,
        render_to_terminal,
        no_stream,
        cancel,
        plain_terminal_stream,
        temperature_override,
        seed_override,
    } = p;
    let filtered: Vec<Message> = messages
        .iter()
        .filter(|m| !is_chat_ui_separator(m))
        .cloned()
        .collect();
    let req = tool_chat_request(
        cfg,
        &filtered,
        tools_defs,
        temperature_override,
        seed_override,
    );
    let (mut msg, finish_reason) = complete_chat_retrying(
        llm_backend,
        client,
        api_key,
        cfg,
        &req,
        out,
        render_to_terminal,
        no_stream,
        cancel,
        plain_terminal_stream,
    )
    .await?;
    crate::text_sanitize::materialize_deepseek_dsml_tool_calls_in_message(&mut msg);
    Ok((msg, finish_reason))
}

// --- R：终答阶段（须含规划等）---

/// R：模型本轮若为最终文本（非 tool_calls），决定是否结束或追加重写提示。
pub(crate) enum ReflectOnAssistantOutcome {
    /// 结束 `run_agent_turn` 外层循环
    StopTurn,
    /// 已写入重写 user 消息，应继续外层循环再次请求模型
    ContinueOuterForPlanRewrite,
    /// 进入工具执行阶段
    ProceedToExecuteTools,
    /// 规划重写次数用尽（已尝试发 SSE 错误码 `plan_rewrite_exhausted`）
    PlanRewriteExhausted,
}

/// 在已将 assistant 推入 `messages` 之后调用，决定是执行工具、终答结束还是规划重写。
///
/// **兼容**：部分 OpenAI 兼容实现在返回 `tool_calls` 时仍上报 `finish_reason: "stop"` 或空串。
/// 若仅判断 `finish_reason == "tool_calls"`，会误判为终答并 `StopTurn`，历史中留下未执行的
/// `tool_calls`、缺对应 `role: tool`，下一轮易 400，且本轮无任何工具执行。故 **非空 `tool_calls`**
/// 同样进入执行分支。
pub(crate) fn per_reflect_after_assistant(
    per_coord: &mut PerCoordinator,
    finish_reason: &str,
    msg: &Message,
    messages: &mut Vec<Message>,
) -> ReflectOnAssistantOutcome {
    if finish_reason == "tool_calls" || msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
        return ReflectOnAssistantOutcome::ProceedToExecuteTools;
    }
    match per_coord.after_final_assistant(msg, messages.as_slice()) {
        super::per_coord::AfterFinalAssistant::StopTurn => ReflectOnAssistantOutcome::StopTurn,
        super::per_coord::AfterFinalAssistant::RequestPlanRewrite(m) => {
            messages.push(m);
            ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite
        }
        super::per_coord::AfterFinalAssistant::StopTurnPlanRewriteExhausted => {
            ReflectOnAssistantOutcome::PlanRewriteExhausted
        }
    }
}

// --- E：执行 tool_calls（Web）---

pub(crate) struct WebExecuteCtx<'a> {
    pub cfg: &'a Arc<AgentConfig>,
    pub effective_working_dir: &'a Path,
    pub workspace_is_set: bool,
    pub out: Option<&'a mpsc::Sender<String>>,
    pub web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    /// 终端 CLI：`run_command` 非白名单时 stdin 审批；`None` 时与历史一致（非白名单则无法执行）。
    pub cli_tool_ctx: Option<&'a tool_registry::CliToolRuntime>,
    /// CLI：`render_to_terminal` 且 `out: None` 时为 true，工具结果打印到 stdout。
    pub echo_terminal_transcript: bool,
}

/// Web/CLI 共用：外层循环与分阶段规划注入共用的一套运行期参数。
pub(crate) struct RunLoopParams<'a> {
    pub llm_backend: &'a (dyn crate::llm::ChatCompletionsBackend + 'static),
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub cfg: &'a Arc<AgentConfig>,
    pub tools_defs: &'a [crate::types::Tool],
    pub messages: &'a mut Vec<Message>,
    pub out: Option<&'a mpsc::Sender<String>>,
    pub effective_working_dir: &'a Path,
    pub workspace_is_set: bool,
    pub no_stream: bool,
    pub cancel: Option<&'a AtomicBool>,
    pub render_to_terminal: bool,
    /// 见 [`crate::llm::api::stream_chat`] 的 `plain_terminal_stream`；仅 CLI 入口为 `true`。
    pub plain_terminal_stream: bool,
    pub web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    /// 与 [`WebExecuteCtx::cli_tool_ctx`] 相同；Web 队列传 `None`。
    pub cli_tool_ctx: Option<&'a tool_registry::CliToolRuntime>,
    pub per_flight: Option<Arc<crate::chat_job_queue::PerTurnFlight>>,
    /// `None` 时使用 `cfg.temperature`。
    pub temperature_override: Option<f32>,
    pub seed_override: LlmSeedOverride,
}

pub(crate) enum ExecuteToolsBatchOutcome {
    /// 本批工具跑完，继续外层循环
    Finished,
    /// SSE 在工具执行中断开
    AbortedSse,
}

/// 单工具：SSE / 终端回显 + 追加 `tool` 与可选反思 `user`（与串行路径一致）。
#[allow(clippy::too_many_arguments)]
async fn emit_tool_result_sse_and_append(
    messages: &mut Vec<Message>,
    out: Option<&mpsc::Sender<String>>,
    echo_terminal_transcript: bool,
    terminal_tool_display_max_chars: usize,
    name: &str,
    args: &str,
    id: &str,
    result: String,
    reflection_inject: Option<serde_json::Value>,
) {
    let args_parsed: Option<serde_json::Value> = serde_json::from_str(args).ok();
    let tool_summary = if let Some(ref parsed) = args_parsed {
        tools::summarize_tool_call_parsed(name, parsed)
    } else {
        tools::summarize_tool_call(name, args)
    };

    if echo_terminal_transcript {
        let _ = crate::runtime::terminal_cli_transcript::print_tool_result_terminal(
            name,
            &result,
            terminal_tool_display_max_chars,
        );
    }

    if let Some(tx) = out {
        let parsed = tool_result::parse_legacy_output(name, &result);
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
        let _ = tx
            .send(encode_message(SsePayload::ToolResult {
                tool_result: ToolResultBody {
                    name: name.to_string(),
                    summary: tool_summary,
                    output: result.clone(),
                    ok: Some(parsed.ok),
                    exit_code: parsed.exit_code,
                    error_code: parsed.error_code,
                    stdout,
                    stderr,
                },
            }))
            .await;
    }

    PerCoordinator::append_tool_result_and_reflection(
        messages,
        id.to_string(),
        result,
        reflection_inject,
    );
}

/// SSE 发送端已关闭（与外层 `run_agent_turn` 早退判断一致）。
pub(crate) fn sse_sender_closed(out: Option<&mpsc::Sender<String>>) -> bool {
    out.is_some_and(|tx| tx.is_closed())
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
        let _ = tx
            .send(encode_message(SsePayload::ToolRunning {
                tool_running: false,
            }))
            .await;
    }
    true
}

/// 统计并行只读批次中去重后的唯一 `(name, args)` 数。
fn dedup_readonly_tool_calls_count(tool_calls: &[ToolCall]) -> usize {
    let mut seen: Vec<(&str, &str)> = Vec::with_capacity(tool_calls.len());
    for tc in tool_calls {
        let key = (tc.function.name.as_str(), tc.function.arguments.as_str());
        if !seen.contains(&key) {
            seen.push(key);
        }
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
    out: Option<&'a mpsc::Sender<String>>,
    echo_terminal_transcript: bool,
    terminal_tool_display_max_chars: usize,
    web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    cli_tool_ctx: Option<&'a tool_registry::CliToolRuntime>,
}

async fn per_execute_tools_common(ctx: ExecuteToolsCommonCtx<'_>) -> ExecuteToolsBatchOutcome {
    let ExecuteToolsCommonCtx {
        tool_calls,
        per_coord,
        messages,
        cfg,
        effective_working_dir,
        workspace_is_set,
        out,
        echo_terminal_transcript,
        terminal_tool_display_max_chars,
        web_tool_ctx,
        cli_tool_ctx,
    } = ctx;
    let mut workspace_changed = false;

    if let Some(tx) = out {
        let _ = tx
            .send(encode_message(SsePayload::ToolRunning {
                tool_running: true,
            }))
            .await;
    }

    if tool_registry::tool_calls_allow_parallel_sync_batch(tool_calls) {
        let dedup_count = dedup_readonly_tool_calls_count(tool_calls);
        info!(
            target: "crabmate",
            "并行执行工具批 count={} unique={}（SyncDefault + 只读 + 非构建锁类）",
            tool_calls.len(),
            dedup_count
        );

        let mut unique_keys: Vec<(String, String)> = Vec::with_capacity(tool_calls.len());
        let mut unique_futs = Vec::new();
        for tc in tool_calls {
            let key = (tc.function.name.clone(), tc.function.arguments.clone());
            if unique_keys.contains(&key) {
                continue;
            }
            unique_keys.push(key);
            let cfg = Arc::clone(cfg);
            let wd = effective_working_dir.to_path_buf();
            let name = tc.function.name.clone();
            let args = tc.function.arguments.clone();
            unique_futs.push(async move {
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
                let result = tokio::task::spawn_blocking(move || {
                    let ctx =
                        tools::tool_context_for(cfg.as_ref(), &cfg.allowed_commands, wd.as_path());
                    tools::run_tool(&tool_name, &tool_args, &ctx)
                })
                .await
                .unwrap_or_else(|e| format!("工具执行 panic：{}", e));
                info!(
                    target: "crabmate",
                    "并行工具完成 tool={} elapsed_ms={}",
                    name,
                    t_tool.elapsed().as_millis()
                );
                (name, args, result)
            });
        }
        let unique_outcomes = join_all(unique_futs).await;
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
            let cached = result_map
                .get(&(tc.function.name.as_str(), tc.function.arguments.as_str()))
                .copied()
                .unwrap_or("")
                .to_string();
            emit_tool_result_sse_and_append(
                messages,
                out,
                echo_terminal_transcript,
                terminal_tool_display_max_chars,
                &tc.function.name,
                &tc.function.arguments,
                &tc.id,
                cached,
                None,
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
                emit_tool_result_sse_and_append(
                    messages,
                    out,
                    echo_terminal_transcript,
                    terminal_tool_display_max_chars,
                    &name,
                    &args,
                    &id,
                    cached.clone(),
                    None,
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
            )
            .await;

            info!(
                target: "crabmate",
                "工具调用完成 tool={} elapsed_ms={}",
                name,
                t_tool.elapsed().as_millis()
            );

            if is_readonly {
                readonly_cache.insert(cache_key, result.clone());
            } else {
                readonly_cache.clear();
            }

            emit_tool_result_sse_and_append(
                messages,
                out,
                echo_terminal_transcript,
                terminal_tool_display_max_chars,
                &name,
                &args,
                &id,
                result,
                reflection_inject,
            )
            .await;
        }
    }

    if let Some(tx) = out {
        if workspace_changed {
            let _ = tx
                .send(encode_message(SsePayload::WorkspaceChanged {
                    workspace_changed: true,
                }))
                .await;
        }
        let _ = tx
            .send(encode_message(SsePayload::ToolRunning {
                tool_running: false,
            }))
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
        out,
        web_tool_ctx,
        cli_tool_ctx,
        echo_terminal_transcript,
    } = ctx;

    per_execute_tools_common(ExecuteToolsCommonCtx {
        tool_calls,
        per_coord,
        messages,
        cfg,
        effective_working_dir,
        workspace_is_set,
        out,
        echo_terminal_transcript,
        terminal_tool_display_max_chars: cfg.command_max_output_len,
        web_tool_ctx,
        cli_tool_ctx,
    })
    .await
}

async fn run_agent_outer_loop(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    'outer: loop {
        if sse_sender_closed(p.out) {
            info!(target: "crabmate", "SSE sender closed, aborting run_agent_turn loop early");
            break;
        }
        if p.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            break;
        }

        let render_to_terminal = p.render_to_terminal;
        super::context_window::prepare_messages_for_model(
            p.llm_backend,
            p.client,
            p.api_key,
            p.cfg.as_ref(),
            p.messages,
        )
        .await?;
        let (msg, finish_reason) = per_plan_call_model_retrying(PerPlanCallModelParams {
            llm_backend: p.llm_backend,
            client: p.client,
            api_key: p.api_key,
            cfg: p.cfg.as_ref(),
            tools_defs: p.tools_defs,
            messages: p.messages,
            out: p.out,
            render_to_terminal,
            no_stream: p.no_stream,
            cancel: p.cancel,
            plain_terminal_stream: p.plain_terminal_stream,
            temperature_override: p.temperature_override,
            seed_override: p.seed_override,
        })
        .await?;
        if let Some(f) = p.per_flight.as_ref() {
            f.awaiting_plan_rewrite_model
                .store(false, Ordering::Relaxed);
        }
        debug!(
            target: "crabmate",
            "模型轮次输出 finish_reason={} message_count_before_push={} assistant_preview={}",
            finish_reason,
            p.messages.len(),
            crate::redact::assistant_message_preview_for_log(&msg)
        );
        push_assistant_merging_trailing_empty_placeholder(p.messages, msg.clone());
        if finish_reason == USER_CANCELLED_FINISH_REASON {
            break;
        }

        match per_reflect_after_assistant(per_coord, &finish_reason, &msg, p.messages) {
            ReflectOnAssistantOutcome::StopTurn => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
                break;
            }
            ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                    f.awaiting_plan_rewrite_model.store(true, Ordering::Relaxed);
                }
                continue 'outer;
            }
            ReflectOnAssistantOutcome::ProceedToExecuteTools => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
            }
            ReflectOnAssistantOutcome::PlanRewriteExhausted => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
                if let Some(tx) = p.out {
                    let _ = tx
                        .send(encode_message(SsePayload::Error(SseErrorBody {
                            error: PerCoordinator::plan_rewrite_exhausted_sse_message().to_string(),
                            code: Some("plan_rewrite_exhausted".to_string()),
                        })))
                        .await;
                }
                break;
            }
        }

        let tool_calls = msg.tool_calls.as_ref().ok_or("无 tool_calls")?;
        let echo_terminal_transcript = render_to_terminal && p.out.is_none();
        let exec_outcome = per_execute_tools_web(
            tool_calls,
            per_coord,
            p.messages,
            WebExecuteCtx {
                cfg: p.cfg,
                effective_working_dir: p.effective_working_dir,
                workspace_is_set: p.workspace_is_set,
                out: p.out,
                web_tool_ctx: p.web_tool_ctx,
                cli_tool_ctx: p.cli_tool_ctx,
                echo_terminal_transcript,
            },
        )
        .await;
        if matches!(exec_outcome, ExecuteToolsBatchOutcome::AbortedSse) {
            break;
        }
        if let Some(f) = p.per_flight.as_ref() {
            f.sync_from_per_coord(per_coord);
        }
    }
    Ok(())
}

async fn run_staged_plan_then_execute_steps(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let render_to_terminal = p.render_to_terminal;
    let echo_terminal_staged = render_to_terminal && p.out.is_none();

    super::context_window::prepare_messages_for_model(
        p.llm_backend,
        p.client,
        p.api_key,
        p.cfg.as_ref(),
        p.messages,
    )
    .await?;

    let instr = p.cfg.staged_plan_phase_instruction.trim();
    let plan_system = if instr.is_empty() {
        staged_plan_phase_instruction_default()
    } else {
        instr.to_string()
    };
    let req = no_tools_chat_request(
        p.cfg.as_ref(),
        &build_single_agent_planner_messages(p.messages, plan_system),
        p.temperature_override,
        p.seed_override,
    );
    run_staged_plan_with_prepared_request(
        p,
        per_coord,
        req,
        render_to_terminal,
        echo_terminal_staged,
        "分阶段规划轮模型输出",
        "规划轮不应调用工具；请关闭 staged_plan_execution 或重试。",
        "分步注入 user（完整正文，供排障与日志）",
        |body| Message {
            role: "user".to_string(),
            content: Some(body),
            reasoning_content: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
    )
    .await
}

fn build_single_agent_planner_messages(messages: &[Message], plan_system: String) -> Vec<Message> {
    let mut out: Vec<Message> = messages
        .iter()
        .filter(|m| !is_chat_ui_separator(m))
        .cloned()
        .collect();
    out.push(Message::system_only(plan_system));
    out
}

fn build_logical_dual_planner_messages(messages: &[Message], plan_system: String) -> Vec<Message> {
    let mut out: Vec<Message> = messages
        .iter()
        .filter(|m| !is_chat_ui_separator(m))
        // 逻辑双 agent：规划器只看用户/助手自然语言上下文，不看 tool 结果正文，
        // 避免工具细节污染任务拆解。
        .filter(|m| m.role != "tool")
        .filter(|m| {
            if m.role != "assistant" {
                return true;
            }
            m.content
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    out.push(Message::system_only(plan_system));
    out
}

#[allow(clippy::too_many_arguments)] // 分阶段流程抽象后的共享执行骨架
async fn run_staged_plan_with_prepared_request<F>(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    req: crate::types::ChatRequest,
    render_to_terminal: bool,
    echo_terminal_staged: bool,
    planning_log_label: &str,
    tool_calls_error_message: &str,
    step_injection_log_label: &str,
    make_step_user_message: F,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    F: Fn(String) -> Message,
{
    let (msg, finish_reason) = complete_chat_retrying(
        p.llm_backend,
        p.client,
        p.api_key,
        p.cfg.as_ref(),
        &req,
        p.out,
        render_to_terminal,
        p.no_stream,
        p.cancel,
        p.plain_terminal_stream,
    )
    .await?;

    debug!(
        target: "crabmate",
        "{} finish_reason={} assistant_preview={}",
        planning_log_label,
        finish_reason,
        crate::redact::assistant_message_preview_for_log(&msg)
    );

    if finish_reason == USER_CANCELLED_FINISH_REASON {
        return Ok(());
    }

    if msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
        if let Some(tx) = p.out {
            let _ = tx
                .send(encode_message(SsePayload::Error(SseErrorBody {
                    error: tool_calls_error_message.to_string(),
                    code: Some("staged_plan_tool_calls".to_string()),
                })))
                .await;
        }
        return Ok(());
    }

    push_assistant_merging_trailing_empty_placeholder(p.messages, msg.clone());

    let content = msg.content.as_deref().unwrap_or("");
    let plan = match super::plan_artifact::parse_agent_reply_plan_v1(content) {
        Ok(plan_v1) => plan_v1,
        Err(_) => {
            if let Some(tx) = p.out {
                let _ = tx
                    .send(encode_message(SsePayload::Error(SseErrorBody {
                        error: "规划轮未解析出合法的 agent_reply_plan v1（需 ```json 围栏或单对象 JSON）。"
                            .to_string(),
                        code: Some("staged_plan_invalid".to_string()),
                    })))
                    .await;
            }
            return Ok(());
        }
    };

    let plan_id = next_staged_plan_id();
    let n = plan.steps.len();
    send_staged_plan_started(p.out, &plan_id, n).await;

    send_staged_plan_notice(
        p.out,
        echo_terminal_staged,
        true,
        staged_plan_queue_summary_text(&plan, 0),
    )
    .await;

    let mut staged_loop_cancelled = false;
    let mut completed_steps = 0usize;
    for (i, step) in plan.steps.iter().enumerate() {
        if sse_sender_closed(p.out) || p.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            staged_loop_cancelled = true;
            break;
        }
        let step_index = i + 1;
        send_staged_plan_step_started(
            p.out,
            &plan_id,
            step.id.trim(),
            step_index,
            n,
            step.description.trim(),
        )
        .await;

        let summary_hint = if step_index == n && n > 1 {
            format!(
                "\n本步为最后一步，终答中请简要列出本轮全部 {} 个步骤的完成情况（可对每步附简短说明）。",
                n
            )
        } else {
            String::new()
        };
        let body = format!(
            "【分步执行 {}/{}】{}{}\n- id: {}\n- 描述: {}",
            step_index,
            n,
            crate::runtime::plan_section::STAGED_STEP_USER_BOILERPLATE,
            summary_hint,
            step.id,
            step.description
        );
        debug!(
            target: "crabmate",
            "{} step={}/{} body_len={} body_preview={}",
            step_injection_log_label,
            i + 1,
            n,
            body.len(),
            crate::redact::preview_chars(&body, crate::redact::MESSAGE_LOG_PREVIEW_CHARS)
        );
        if echo_terminal_staged {
            let _ = crate::runtime::terminal_cli_transcript::print_staged_plan_notice(false, &body);
        }
        p.messages.push(make_step_user_message(body));
        let run_step = run_agent_outer_loop(p, per_coord).await;
        if let Err(e) = run_step {
            send_staged_plan_step_finished(
                p.out,
                &plan_id,
                step.id.trim(),
                step_index,
                n,
                "failed",
            )
            .await;
            send_staged_plan_finished(p.out, &plan_id, n, completed_steps, "failed").await;
            return Err(e);
        }
        if sse_sender_closed(p.out) || p.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            send_staged_plan_step_finished(
                p.out,
                &plan_id,
                step.id.trim(),
                step_index,
                n,
                "cancelled",
            )
            .await;
            staged_loop_cancelled = true;
            break;
        }
        send_staged_plan_step_finished(p.out, &plan_id, step.id.trim(), step_index, n, "ok").await;
        completed_steps = step_index;
        p.messages.push(Message::chat_ui_separator(true));
        // 先发队列摘要 SSE，再追加 UI 分隔。
        send_staged_plan_notice(
            p.out,
            echo_terminal_staged,
            true,
            staged_plan_queue_summary_text(&plan, step_index),
        )
        .await;
        emit_chat_ui_separator_sse(p.out, true).await;
    }
    // 末步成功后循环内已发送含「[✓] 全部完成」的摘要，勿再发一次（否则重复一条）。
    send_staged_plan_finished(
        p.out,
        &plan_id,
        n,
        completed_steps,
        if staged_loop_cancelled {
            "cancelled"
        } else {
            "ok"
        },
    )
    .await;
    // 仅当循环内未添加过分隔符时再追加：n==0 未进入循环；或取消时 completed_steps==0。
    // 否则末步成功后已在循环内添加，再加会重复两行。
    if n == 0 || (staged_loop_cancelled && completed_steps == 0) {
        p.messages.push(Message::chat_ui_separator(true));
        emit_chat_ui_separator_sse(p.out, true).await;
    }
    Ok(())
}

async fn run_logical_dual_agent_then_execute_steps(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let render_to_terminal = p.render_to_terminal;
    let echo_terminal_staged = render_to_terminal && p.out.is_none();

    super::context_window::prepare_messages_for_model(
        p.llm_backend,
        p.client,
        p.api_key,
        p.cfg.as_ref(),
        p.messages,
    )
    .await?;

    let instr = p.cfg.staged_plan_phase_instruction.trim();
    let plan_system = if instr.is_empty() {
        staged_plan_phase_instruction_default()
    } else {
        instr.to_string()
    };
    let req = no_tools_chat_request(
        p.cfg.as_ref(),
        &build_logical_dual_planner_messages(p.messages, plan_system),
        p.temperature_override,
        p.seed_override,
    );
    run_staged_plan_with_prepared_request(
        p,
        per_coord,
        req,
        render_to_terminal,
        echo_terminal_staged,
        "逻辑双agent规划轮输出",
        "规划轮不应调用工具；请检查 planner_executor_mode 配置或重试。",
        "逻辑双agent注入执行器user",
        Message::user_only,
    )
    .await
}

pub(crate) async fn run_agent_turn_common(
    p: &mut RunLoopParams<'_>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    debug!(
        target: "crabmate",
        "run_agent_turn 开始 message_count={} last_user_preview={} staged_plan={} planner_executor_mode={} work_dir={}",
        p.messages.len(),
        crate::redact::last_user_message_preview_for_log(p.messages),
        p.cfg.staged_plan_execution,
        p.cfg.planner_executor_mode.as_str(),
        p.effective_working_dir.display()
    );
    insert_separator_after_last_user_for_turn(p.messages);

    let mut per_coord = PerCoordinator::new(
        p.cfg.reflection_default_max_rounds,
        p.cfg.final_plan_requirement,
        p.cfg.plan_rewrite_max_attempts,
    );

    if p.cfg.planner_executor_mode == PlannerExecutorMode::LogicalDualAgent {
        run_logical_dual_agent_then_execute_steps(p, &mut per_coord).await
    } else if p.cfg.staged_plan_execution {
        run_staged_plan_then_execute_steps(p, &mut per_coord).await
    } else {
        run_agent_outer_loop(p, &mut per_coord).await
    }
}

#[cfg(test)]
mod push_assistant_merge_tests {
    use super::{
        build_logical_dual_planner_messages, build_single_agent_planner_messages,
        push_assistant_merging_trailing_empty_placeholder,
    };
    use crate::types::Message;

    fn empty_assistant() -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(String::new()),
            reasoning_content: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    fn assistant_body(s: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(s.to_string()),
            reasoning_content: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn replaces_trailing_empty_assistant_placeholder() {
        let mut m = vec![Message::user_only("hi"), empty_assistant()];
        push_assistant_merging_trailing_empty_placeholder(&mut m, assistant_body("plan"));
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].content.as_deref(), Some("plan"));
    }

    #[test]
    fn pushes_when_last_assistant_has_content() {
        let mut m = vec![Message::user_only("hi"), assistant_body("first")];
        push_assistant_merging_trailing_empty_placeholder(&mut m, assistant_body("second"));
        assert_eq!(m.len(), 3);
        assert_eq!(m[2].content.as_deref(), Some("second"));
    }

    #[test]
    fn planner_messages_single_agent_keeps_tool_results() {
        let src = vec![
            Message::system_only("sys"),
            Message::user_only("u1"),
            assistant_body("a1"),
            Message {
                role: "tool".to_string(),
                content: Some("tool out".to_string()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
        let out = build_single_agent_planner_messages(&src, "plan sys".to_string());
        assert_eq!(out.len(), 5);
        assert_eq!(out[3].role, "tool");
        assert_eq!(out[4].role, "system");
        assert_eq!(out[4].content.as_deref(), Some("plan sys"));
    }

    #[test]
    fn planner_messages_logical_dual_drops_tool_and_empty_assistant() {
        let src = vec![
            Message::system_only("sys"),
            Message::user_only("u1"),
            assistant_body("a1"),
            empty_assistant(),
            Message {
                role: "tool".to_string(),
                content: Some("tool out".to_string()),
                reasoning_content: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
        let out = build_logical_dual_planner_messages(&src, "plan sys".to_string());
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].role, "system");
        assert_eq!(out[1].role, "user");
        assert_eq!(out[2].role, "assistant");
        assert_eq!(out[3].role, "system");
        assert_eq!(out[3].content.as_deref(), Some("plan sys"));
        assert!(!out.iter().any(|m| m.role == "tool"));
    }
}

#[cfg(test)]
mod dedup_tool_calls_tests {
    use super::dedup_readonly_tool_calls_count;
    use crate::types::{FunctionCall, ToolCall};

    fn tc(id: &str, name: &str, args: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: name.to_string(),
                arguments: args.to_string(),
            },
        }
    }

    #[test]
    fn no_duplicates() {
        let calls = vec![
            tc("1", "read_file", r#"{"path":"a.txt"}"#),
            tc("2", "list_dir", r#"{"path":"."}"#),
        ];
        assert_eq!(dedup_readonly_tool_calls_count(&calls), 2);
    }

    #[test]
    fn identical_calls_deduped() {
        let calls = vec![
            tc("1", "read_file", r#"{"path":"a.txt"}"#),
            tc("2", "read_file", r#"{"path":"a.txt"}"#),
            tc("3", "read_file", r#"{"path":"a.txt"}"#),
        ];
        assert_eq!(dedup_readonly_tool_calls_count(&calls), 1);
    }

    #[test]
    fn same_name_different_args_not_deduped() {
        let calls = vec![
            tc("1", "read_file", r#"{"path":"a.txt"}"#),
            tc("2", "read_file", r#"{"path":"b.txt"}"#),
        ];
        assert_eq!(dedup_readonly_tool_calls_count(&calls), 2);
    }

    #[test]
    fn mixed_duplicates() {
        let calls = vec![
            tc("1", "read_file", r#"{"path":"a.txt"}"#),
            tc("2", "list_dir", r#"{"path":"."}"#),
            tc("3", "read_file", r#"{"path":"a.txt"}"#),
            tc("4", "grep", r#"{"pattern":"foo"}"#),
            tc("5", "list_dir", r#"{"path":"."}"#),
        ];
        assert_eq!(dedup_readonly_tool_calls_count(&calls), 3);
    }

    #[test]
    fn empty_batch() {
        assert_eq!(dedup_readonly_tool_calls_count(&[]), 0);
    }
}

#[cfg(test)]
mod per_reflect_tests {
    use super::{ReflectOnAssistantOutcome, per_reflect_after_assistant};
    use crate::agent::per_coord::{FinalPlanRequirementMode, PerCoordinator};
    use crate::types::{FunctionCall, Message, ToolCall};

    #[test]
    fn proceed_to_tools_when_tool_calls_present_but_finish_reason_stop() {
        let mut c = PerCoordinator::new(5, FinalPlanRequirementMode::Never, 3);
        let mut messages = vec![Message::user_only("x")];
        let msg = Message {
            role: "assistant".to_string(),
            content: Some("ok".to_string()),
            reasoning_content: None,
            tool_calls: Some(vec![ToolCall {
                id: "1".into(),
                typ: "function".into(),
                function: FunctionCall {
                    name: "create_file".into(),
                    arguments: "{}".into(),
                },
            }]),
            name: None,
            tool_call_id: None,
        };
        let out = per_reflect_after_assistant(&mut c, "stop", &msg, &mut messages);
        assert!(matches!(
            out,
            ReflectOnAssistantOutcome::ProceedToExecuteTools
        ));
    }
}
