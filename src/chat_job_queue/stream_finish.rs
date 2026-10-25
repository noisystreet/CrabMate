//! 流式任务在 `run_agent_turn` 之后的收尾：落盘、SSE 终端帧、`final_response` 兜底等。

use std::sync::Arc;

use crate::cm_sse_protocol::StreamEndReason;
use log::{debug, error, info, warn};
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};

use crate::agent::agent_turn::AgentTurnJobOutcomeKind;
use crate::agent_role_turn::persisted_agent_role_after_turn;
use crate::config::AgentConfig;
use crate::sse::SseStreamHub;
use crate::types::{Message, assistant_output_kind, message_content_as_str};
use crate::web::WebChatJobAppFacet;

use super::WebChatQueueDeps;

/// [`post_turn_web_prepare_and_save`] 入参（避免过长形参列表）。
pub(super) struct PostTurnWebPrepareParams<'a> {
    pub(super) app: &'a WebChatJobAppFacet,
    pub(super) queue_deps: &'a WebChatQueueDeps,
    pub(super) cfg_snap: &'a Arc<AgentConfig>,
    pub(super) conversation_id: &'a str,
    pub(super) messages: &'a mut Vec<Message>,
    pub(super) expected_revision: Option<u64>,
    pub(super) request_agent_role: Option<&'a str>,
    pub(super) persisted_active_agent_role: Option<&'a str>,
    pub(super) request_session_mode: Option<&'a str>,
    pub(super) persisted_active_session_mode: Option<&'a str>,
    /// 本请求显式申请的权限等级（落盘时优先于 `persisted_active_permission_level`）。
    pub(super) request_permission_level: Option<&'a str>,
    /// 回合开始前持久化的权限等级意图。
    pub(super) persisted_active_permission_level: Option<&'a str>,
    /// 本轮结构性提交结果（A2/A3）：canonical 收缩区间，作为归档行落盘。
    pub(super) structural_commit: crate::agent::model_context_view::StructuralCommit,
}

/// 落盘 / 结构性提交前的注入净化链（ADR §3.7.1，必须显式复用、不得依赖隐式副作用）。
///
/// 契约：
/// - [`crate::memory::long_term_memory::strip_long_term_memory_injections`] — 长期记忆 `CRABMATE_LONG_TERM_MEMORY_NAME`；
/// - [`crate::workspace::changelist::strip_workspace_changelist_injections`] — 变更集 `CRABMATE_WORKSPACE_CHANGELIST_NAME`；
/// - [`crate::types::strip_orchestration_injected_users_for_conversation_store`] — 其余编排注入 user
///   （**保留**基线摘要 `CRABMATE_CONTEXT_SUMMARY_NAME` 与首轮工作区画像 `CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME`）；
/// - [`crate::types::is_execution_constraint_ephemeral_system`] 的执行约束临时 system 正常由
///   `outer_loop` 每轮 retain 移出，异常残留须在此一并剔除。
pub(crate) fn strip_persist_injections(messages: &mut Vec<Message>) {
    crate::memory::long_term_memory::strip_long_term_memory_injections(messages);
    crate::workspace::changelist::strip_workspace_changelist_injections(messages);
    crate::types::strip_orchestration_injected_users_for_conversation_store(messages);
    messages.retain(|m| !crate::types::is_execution_constraint_ephemeral_system(m));
}

/// Web 队列：`run_agent_turn` 成功后的 LTM 异步索引、剥离注入与会话按 revision 落盘。
pub(super) async fn post_turn_web_prepare_and_save(
    p: PostTurnWebPrepareParams<'_>,
) -> crate::SaveConversationOutcome {
    let PostTurnWebPrepareParams {
        app,
        queue_deps,
        cfg_snap,
        conversation_id,
        messages,
        expected_revision,
        request_agent_role,
        persisted_active_agent_role,
        request_session_mode,
        persisted_active_session_mode,
        request_permission_level,
        persisted_active_permission_level,
        structural_commit,
    } = p;
    let scope = conversation_id.to_string();
    let to_index = messages.clone();
    if let (Some(ltm), true) = (
        queue_deps.long_term_memory.as_ref(),
        cfg_snap.long_term_memory.long_term_memory_enabled,
    ) {
        ltm.clone()
            .spawn_turn_memory_postprocess(Arc::clone(cfg_snap), scope, to_index);
    }
    strip_persist_injections(messages);
    let active_save =
        persisted_agent_role_after_turn(persisted_active_agent_role, request_agent_role);
    let mode_save = crate::session_mode_turn::persisted_session_mode_after_turn(
        persisted_active_session_mode,
        request_session_mode,
    );
    // 只存**意图**：生效值每回合按 min 链重算（运维调低上限后旧会话才会跟着降级）。
    let level_save = crate::cm_types::persisted_permission_level_after_turn(
        persisted_active_permission_level,
        request_permission_level,
    );
    let archive_rows = crate::conversation_store::archive_rows_from_structural_commit(&structural_commit);
    app.conversation
        .save_conversation_messages_if_revision(
            conversation_id.to_string(),
            messages.clone(),
            active_save.as_deref(),
            mode_save.as_deref(),
            level_save.as_deref(),
            expected_revision,
            archive_rows,
        )
        .await
}

/// 取消 / 中断回合落盘入参（避免过长形参列表）。
pub(super) struct CancelledTurnSaveParams<'a> {
    pub(super) app: &'a WebChatJobAppFacet,
    pub(super) conversation_id: &'a str,
    pub(super) messages: &'a mut Vec<Message>,
    pub(super) expected_revision: Option<u64>,
    pub(super) request_agent_role: Option<&'a str>,
    pub(super) persisted_active_agent_role: Option<&'a str>,
    pub(super) request_session_mode: Option<&'a str>,
    pub(super) persisted_active_session_mode: Option<&'a str>,
    /// 本请求显式申请的权限等级（落盘时优先于 `persisted_active_permission_level`）。
    pub(super) request_permission_level: Option<&'a str>,
    /// 回合开始前持久化的权限等级意图。
    pub(super) persisted_active_permission_level: Option<&'a str>,
    pub(super) job_id: u64,
    /// 归档行：`Ok(commit)` 取消分支须传结构性提交的归档行（本轮若发生压缩，canonical 已收缩，
    /// 归档行丢了就是数据丢失）；`Err` 取消分支只做仅追加写回，传空。
    pub(super) archive_rows: Vec<crate::conversation_store::ArchiveRow>,
}

/// 取消 / 中断路径的**尽力落盘**：把本轮已产出的部分输出写入会话 store，
/// 使下一轮模型上下文包含「我已产出这些、被用户中断」。
///
/// 与 [`post_turn_web_prepare_and_save`] 的刻意差异：
/// - **跳过** LTM 异步索引（本轮未正常完成，不索引半成品）；
/// - **不**发 `conversation_saved` / state snapshot / `stream_draining`——取消路径的 SSE 语义保持
///   不变（worker 随后只补 `STREAM_CANCELLED`）；
/// - 本轮无可见助手输出时**不落盘**（空回合不写 store）；可见性按**结构修复前**的形态
///   判定，因此「只有半截 `tool_calls`、没有正文」的取消回合仍会落盘（用户消息得以保留）；
/// - CAS `Conflict` 仅记日志，不当成取消失败。
pub(super) async fn save_cancelled_turn_best_effort(p: CancelledTurnSaveParams<'_>) {
    let CancelledTurnSaveParams {
        app,
        conversation_id,
        messages,
        expected_revision,
        request_agent_role,
        persisted_active_agent_role,
        request_session_mode,
        persisted_active_session_mode,
        request_permission_level,
        persisted_active_permission_level,
        job_id,
        archive_rows,
    } = p;
    strip_persist_injections(messages);
    // 可见性按**修复前**的形态判定：取消常停在 `assistant(tool_calls=[半截 arguments])`
    // 且本轮没有正文，修复会丢弃该残片使助手变空。若修复后再判定，这一轮（连同本轮
    // user 消息）会被整体跳过落盘，用户在服务端历史里丢失自己的输入。修复后留下的空
    // 助手出站时由 `normalize_messages_for_openai_compatible_request` 剔除，安全。
    let had_visible_output = current_turn_has_visible_assistant_output(messages);
    // 取消可能停在 `assistant(tool_calls=[A, B]) + tool(A)` 或半截 arguments 上；
    // 落盘前修复结构，避免下一轮请求被上游以「tool_calls 缺少 tool 响应」拒绝。
    let repaired = crate::cm_agent::message_pipeline::
        complete_pending_tool_responses_for_persistence(messages);
    if repaired > 0 {
        info!(
            target: "crabmate",
            "chat stream 取消回合落盘前补齐缺失的 tool 响应 job_id={} added={}",
            job_id,
            repaired
        );
    }
    if !had_visible_output {
        return;
    }
    let active_save =
        persisted_agent_role_after_turn(persisted_active_agent_role, request_agent_role);
    let mode_save = crate::session_mode_turn::persisted_session_mode_after_turn(
        persisted_active_session_mode,
        request_session_mode,
    );
    let level_save = crate::cm_types::persisted_permission_level_after_turn(
        persisted_active_permission_level,
        request_permission_level,
    );
    match app
        .conversation
        .save_conversation_messages_if_revision(
            conversation_id.to_string(),
            messages.clone(),
            active_save.as_deref(),
            mode_save.as_deref(),
            level_save.as_deref(),
            expected_revision,
            archive_rows,
        )
        .await
    {
        crate::SaveConversationOutcome::Saved => info!(
            target: "crabmate",
            "chat stream 取消回合已落盘部分输出 job_id={}",
            job_id
        ),
        crate::SaveConversationOutcome::Conflict => warn!(
            target: "crabmate",
            "chat stream 取消回合落盘冲突（会话已被并发更新，本轮部分输出未落盘）job_id={} conversation_id={} expected_revision={:?}",
            job_id,
            conversation_id,
            expected_revision
        ),
    }
}

/// 流任务被取消且 **mpsc 仍有接收端** 时补发一条带 `code: STREAM_CANCELLED` 的控制面，便于前端与代理统一收尾（接收端已 drop 时仅 debug，避免误报）。
pub(crate) async fn emit_stream_cancelled_terminal(
    sse_tx: &mpsc::Sender<String>,
    job_id: u64,
    request_id: Option<String>,
) {
    if sse_tx.is_closed() {
        debug!(
            target: "crabmate",
            "stream 任务已取消且 SSE 已无接收端，跳过 STREAM_CANCELLED 帧 job_id={}",
            job_id
        );
        return;
    }
    let line = crate::sse::encode_message(crate::sse::SsePayload::Error(
        crate::sse::SseErrorBody {
            error: "流已取消".to_string(),
            code: Some(crate::types::SSE_STREAM_CANCELLED_CODE.to_string()),
            reason_code: None,
            turn_id: Some(job_id),
            sub_phase: None,
            request_id: None,
        }
        .with_request_id(request_id),
    ));
    if crate::sse::send_string_logged(
        sse_tx,
        line,
        "chat_job_queue::emit_stream_cancelled_terminal",
    )
    .await
    {
        debug!(
            target: "crabmate",
            "stream 已下发 STREAM_CANCELLED 控制帧 job_id={}",
            job_id
        );
    }
}

pub(crate) async fn emit_stream_ended_once(
    sse_tx: &mpsc::Sender<String>,
    job_id: u64,
    reason: StreamEndReason,
    stream_ended_sent: &mut bool,
    log_context: &'static str,
    tiktoken_prompt_tokens: Option<crate::types::TiktokenPromptTokensSnapshot>,
) {
    if *stream_ended_sent {
        return;
    }
    let end_line = crate::sse::encode_message(crate::sse::SsePayload::StreamEnded {
        ended: crate::sse::StreamEndedBody {
            job_id,
            reason,
            tiktoken_prompt_tokens,
        },
    });
    let _ = crate::sse::send_string_logged(sse_tx, end_line, log_context).await;
    *stream_ended_sent = true;
}

/// 非终态：模型/工具已结束、正在落盘；官方 Web 可提前进入 Draining 文案。
pub(crate) async fn emit_stream_draining(sse_tx: &mpsc::Sender<String>, job_id: u64) {
    let line = crate::sse::encode_message(crate::sse::SsePayload::StreamDraining {
        draining: crate::sse::StreamDrainingBody { job_id },
    });
    let _ = crate::sse::send_string_logged(sse_tx, line, "chat_job_queue::stream stream_draining")
        .await;
}

pub(crate) fn sse_payload_has_final_response_timeline(payload: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else {
        return false;
    };
    // V1 格式：{"v":2,"timeline_log":{"kind":"final_response",...}}
    if v.get("timeline_log")
        .and_then(|x| x.as_object())
        .and_then(|obj| obj.get("kind"))
        .and_then(|x| x.as_str())
        .is_some_and(|k| k == "final_response")
    {
        return true;
    }
    // V2（AG-UI）格式：{"type":"CUSTOM","customType":"timeline_log","data":{"kind":"final_response",...}}
    if v.get("type").and_then(|x| x.as_str()) == Some("CUSTOM")
        && v.get("customType").and_then(|x| x.as_str()) == Some("timeline_log")
    {
        return v
            .get("data")
            .and_then(|d| d.get("kind"))
            .and_then(|x| x.as_str())
            .is_some_and(|k| k == "final_response");
    }
    false
}

fn stream_job_has_final_response_timeline(hub: &SseStreamHub, job_id: u64) -> bool {
    hub.replay_after(job_id, 0)
        .unwrap_or_default()
        .into_iter()
        .any(|(_, payload)| sse_payload_has_final_response_timeline(&payload))
}

async fn stream_job_has_final_response_timeline_eventually(
    hub: &SseStreamHub,
    job_id: u64,
) -> bool {
    const MAX_RETRIES: usize = 5;
    const RETRY_DELAY_MS: u64 = 20;
    for attempt in 0..=MAX_RETRIES {
        if stream_job_has_final_response_timeline(hub, job_id) {
            return true;
        }
        if attempt < MAX_RETRIES {
            sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
        }
    }
    false
}

/// 层 4 兜底终答取自思维链时，向用户标明来源（该文本此前已随流式思维链下发过）。
const REASONING_FALLBACK_PREFIX: &str =
    "（本轮模型未输出正文，以下为从思考内容回填的兜底内容）\n\n";
/// 层 4 兜底终答取自思维链时的字符上限，超出即截断并追加省略号（避免整段 CoT 外泄）。
const REASONING_FALLBACK_MAX_CHARS: usize = 2000;

fn reasoning_fallback_text(reasoning: &str) -> String {
    let mut out = String::from(REASONING_FALLBACK_PREFIX);
    let mut chars = reasoning.chars();
    out.extend(chars.by_ref().take(REASONING_FALLBACK_MAX_CHARS));
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

fn last_assistant_text_for_fallback(messages: &[Message]) -> Option<String> {
    let range_start = messages
        .iter()
        .rposition(|m| m.role == "user")
        .map(|idx| idx.saturating_add(1))
        .unwrap_or(0);
    let last = messages
        .iter()
        .skip(range_start)
        .rev()
        .find(|m| m.role == "assistant")?;
    if let Some(text) = message_content_as_str(&last.content)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(text.to_string());
    }
    // 层 4：末条助手仅有思维链（`ReasoningOnly`，无正文 / 无工具调用）时，用思维链作为兜底
    // 终答，避免「`completed` + 无 `assistant_answer_phase` + 无 `final_response`」矛盾收尾。
    // 加来源前缀并截断：思维链已随流式下发，直接复用会重复展示且外泄 CoT。
    last.reasoning_content
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(reasoning_fallback_text)
}

fn current_turn_has_visible_assistant_output(messages: &[Message]) -> bool {
    let range_start = messages
        .iter()
        .rposition(|m| m.role == "user")
        .map(|idx| idx.saturating_add(1))
        .unwrap_or(0);
    // 层 2：与 `stream_end_reason_from_finish_and_message` 共用同一判定；仅思维链不算可见输出。
    messages
        .iter()
        .skip(range_start)
        .any(|m| assistant_output_kind(m).counts_as_visible_output())
}

/// 层 2/4/1：一轮成功收尾时的 `stream_ended` 终态判定（单一事实来源，便于单独测试）。
///
/// - `truncated==true` → [`StreamEndReason::Truncated`]（层 1：上游 `finish_reason=="length"` 或流被切断）；
/// - `fallback_emitted==true` → [`StreamEndReason::Fallback`]（层 4 兜底已补发终答）；
/// - 否则本轮存在可见助手输出（正文或工具调用）→ [`StreamEndReason::Completed`]；
/// - 否则（仅思维链 / 空）→ [`StreamEndReason::NoOutput`]。
pub(crate) fn completed_turn_end_reason(
    fallback_emitted: bool,
    truncated: bool,
    messages: &[Message],
) -> StreamEndReason {
    if truncated {
        StreamEndReason::Truncated
    } else if fallback_emitted {
        StreamEndReason::Fallback
    } else if current_turn_has_visible_assistant_output(messages) {
        StreamEndReason::Completed
    } else {
        StreamEndReason::NoOutput
    }
}

pub(crate) async fn emit_missing_final_response_fallback_if_needed(
    hub: &SseStreamHub,
    sse_tx: &mpsc::Sender<String>,
    job_id: u64,
    messages: &[Message],
) -> bool {
    if stream_job_has_final_response_timeline_eventually(hub, job_id).await {
        return false;
    }
    let Some(final_text) = last_assistant_text_for_fallback(messages) else {
        return false;
    };
    debug!(
        target: "crabmate",
        "stream compatibility fallback: missing final_response timeline, emit synthesized terminal frame job_id={}",
        job_id
    );
    let message_id = "msg-fallback";
    // 关闭 reasoning 生命周期，开启 text 生命周期
    crate::sse::send_reasoning_message_end_sse(sse_tx, "reasoning").await;
    crate::sse::send_text_message_start_sse(sse_tx, message_id, "assistant").await;
    crate::sse::send_final_response_timeline_then_answer_phase(
        sse_tx,
        final_text,
        "chat_job_queue::stream final_response_fallback",
        "chat_job_queue::stream answer_phase_fallback",
    )
    .await;
    crate::sse::send_text_message_end_sse(sse_tx, message_id).await;
    true
}

/// `run_agent_turn` 之后的流式任务收尾：落盘、SSE 错误/冲突、`stream_ended` 等。
pub(super) struct StreamJobOutcomeCtx<'a> {
    pub(super) r: crate::turn_runner::TurnRunOutcome,
    pub(super) cancelled_by_signal: bool,
    /// 层 1：本轮最后一次 assistant 输出是否被截断（来自 [`crate::per_turn_flight::PerTurnFlight`]）。
    pub(super) truncated: bool,
    pub(super) queue_deps: &'a WebChatQueueDeps,
    pub(super) sse_tx: &'a mpsc::Sender<String>,
    pub(super) job_id: u64,
    pub(super) request_id: Option<String>,
    pub(super) messages: &'a mut Vec<Message>,
    pub(super) cfg_snap: &'a Arc<AgentConfig>,
    pub(super) app: &'a WebChatJobAppFacet,
    pub(super) conversation_id: &'a str,
    pub(super) expected_revision: Option<u64>,
    pub(super) request_agent_role: Option<&'a str>,
    pub(super) persisted_active_agent_role: Option<&'a str>,
    pub(super) request_session_mode: Option<&'a str>,
    pub(super) persisted_active_session_mode: Option<&'a str>,
    /// 本请求显式申请的权限等级；与 `persisted_active_permission_level` 一起决定落盘意图。
    pub(super) request_permission_level: Option<&'a str>,
    pub(super) persisted_active_permission_level: Option<&'a str>,
    pub(super) stream_ended_sent: &'a mut bool,
}

pub(crate) async fn stream_job_outcome_after_agent_turn(
    ctx: StreamJobOutcomeCtx<'_>,
) -> (bool, bool, Option<String>, StreamEndReason) {
    let StreamJobOutcomeCtx {
        r,
        cancelled_by_signal,
        truncated,
        queue_deps,
        sse_tx,
        job_id,
        request_id,
        messages,
        cfg_snap,
        app,
        conversation_id,
        expected_revision,
        request_agent_role,
        persisted_active_agent_role,
        request_session_mode,
        persisted_active_session_mode,
        request_permission_level,
        persisted_active_permission_level,
        stream_ended_sent,
    } = ctx;
    match r {
        Ok(commit) if cancelled_by_signal => {
            info!(target: "crabmate", "chat stream 任务已取消 job_id={}", job_id);
            // 本轮 partial 输出已在 `messages`（agent 已 push + 提交视图）；落盘使下一轮上下文可见。
            save_cancelled_turn_best_effort(CancelledTurnSaveParams {
                app,
                conversation_id,
                messages,
                expected_revision,
                request_agent_role,
                persisted_active_agent_role,
                request_session_mode,
                persisted_active_session_mode,
                request_permission_level,
                persisted_active_permission_level,
                job_id,
                archive_rows: crate::conversation_store::archive_rows_from_structural_commit(
                    &commit,
                ),
            })
            .await;
            (false, true, None, StreamEndReason::Cancelled)
        }
        Ok(commit) => {
            let fallback_emitted = emit_missing_final_response_fallback_if_needed(
                queue_deps.sse_stream_hub.as_ref(),
                sse_tx,
                job_id,
                messages,
            )
            .await;
            let end_reason = completed_turn_end_reason(fallback_emitted, truncated, messages);
            let tiktoken_prompt_tokens =
                crate::agent::tiktoken_prompt_tokens::prompt_token_count_vendor_shaped_for_session(
                    cfg_snap, messages,
                );
            // Phase E1：先发非终态 `stream_draining`（解除「模型生成中」），再落盘 →
            // `conversation_saved` → 可选 snapshot → **最后** `RUN_FINISHED`。
            emit_stream_draining(sse_tx, job_id).await;
            match post_turn_web_prepare_and_save(PostTurnWebPrepareParams {
                app,
                queue_deps,
                cfg_snap,
                conversation_id,
                messages,
                expected_revision,
                request_agent_role,
                persisted_active_agent_role,
                request_session_mode,
                persisted_active_session_mode,
                request_permission_level,
                persisted_active_permission_level,
                structural_commit: commit,
            })
            .await
            {
                crate::SaveConversationOutcome::Saved => {
                    if let Some(new_rev) = app
                        .conversation
                        .load_conversation_seed(conversation_id)
                        .await
                        .and_then(|s| s.expected_revision)
                    {
                        let line =
                            crate::sse::encode_message(crate::sse::SsePayload::ConversationSaved {
                                saved: crate::sse::ConversationSavedBody {
                                    revision: new_rev,
                                    tiktoken_prompt_tokens: tiktoken_prompt_tokens.clone(),
                                },
                            });
                        let _ = crate::sse::send_string_logged(
                            sse_tx,
                            line,
                            "chat_job_queue::stream conversation_saved",
                        )
                        .await;
                    }
                    // 落盘之后：已 strip 注入，形状贴近会话 store（非流式中途视图）。
                    let snapshot_state = serde_json::json!({
                        "phase": "stream_ended",
                        "messages": messages.iter().map(|m| {
                            serde_json::json!({
                                "role": m.role,
                                "content": crate::types::message_content_as_str(&m.content),
                                "reasoning": m.reasoning_content,
                                "tool_calls": m.tool_calls,
                            })
                        }).collect::<Vec<_>>(),
                    });
                    crate::sse::send_state_snapshot_sse(sse_tx, snapshot_state).await;
                    emit_stream_ended_once(
                        sse_tx,
                        job_id,
                        end_reason,
                        stream_ended_sent,
                        "chat_job_queue::stream stream_ended",
                        tiktoken_prompt_tokens.clone(),
                    )
                    .await;
                    (true, false, None, end_reason)
                }
                crate::SaveConversationOutcome::Conflict => {
                    // 勿先发成功终态：冲突错误在前，worker 再发 `RUN_FINISHED`(conflict)。
                    let err_line = crate::conversation_conflict_sse_line(request_id.clone());
                    let _ = crate::sse::send_string_logged(
                        sse_tx,
                        err_line,
                        "chat_job_queue::stream conversation_conflict",
                    )
                    .await;
                    (
                        false,
                        false,
                        Some("conversation_conflict".to_string()),
                        StreamEndReason::Conflict,
                    )
                }
            }
        }
        Err(e) => {
            let e_text = e.to_string();
            match e.job_queue_stream_outcome_kind(cancelled_by_signal) {
                AgentTurnJobOutcomeKind::UserCancelled => {
                    info!(
                        target: "crabmate",
                        "chat stream 任务已取消 job_id={} reason={}",
                        job_id,
                        e_text
                    );
                    // `run_agent_turn` 已在 Err 中断路径把本轮部分输出**仅追加**写回 `messages`
                    // （无压缩 ⇒ 无归档行）；此处落盘使下一轮上下文可见。
                    save_cancelled_turn_best_effort(CancelledTurnSaveParams {
                        app,
                        conversation_id,
                        messages,
                        expected_revision,
                        request_agent_role,
                        persisted_active_agent_role,
                        request_session_mode,
                        persisted_active_session_mode,
                        request_permission_level,
                        persisted_active_permission_level,
                        job_id,
                        archive_rows: Vec::new(),
                    })
                    .await;
                    (false, true, None, StreamEndReason::Cancelled)
                }
                AgentTurnJobOutcomeKind::FailureEmitSseError => {
                    error!(
                        target: "crabmate",
                        "chat stream 任务失败 job_id={} err_kind=agent_turn {}",
                        job_id,
                        e.diag_log_kv(),
                    );
                    let err_body =
                        e.sse_error_payload_with_request_id(Some(job_id), request_id.clone());
                    let err_line =
                        crate::sse::encode_message(crate::sse::SsePayload::Error(err_body));
                    let _ = crate::sse::send_string_logged(
                        sse_tx,
                        err_line,
                        "chat_job_queue::stream agent_turn_error",
                    )
                    .await;
                    (
                        false,
                        false,
                        e.short_detail_for_job_log(),
                        StreamEndReason::NoOutput,
                    )
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{current_turn_has_visible_assistant_output, strip_persist_injections};
    use crate::types::{
        CRABMATE_CONTEXT_SUMMARY_NAME, CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME,
        CRABMATE_LONG_TERM_MEMORY_NAME, CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME,
        CRABMATE_WORKSPACE_CHANGELIST_NAME, FunctionCall, Message, ToolCall,
    };

    /// ADR §3.7.1：净化链**保留**基线摘要与首轮工作区画像，**剔除**其余注入。
    #[test]
    fn strip_persist_injections_keeps_summary_and_profile_drops_rest() {
        let mut messages = vec![
            Message::user_only("真实问题"),
            Message::user_server_injection(CRABMATE_LONG_TERM_MEMORY_NAME, "长期记忆注入"),
            Message::user_server_injection(CRABMATE_WORKSPACE_CHANGELIST_NAME, "变更集注入"),
            Message::user_server_injection(CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME, "规划轮拒绝重写"),
            Message::user_context_summary_injection("基线摘要"),
            Message::user_first_turn_workspace_context("首轮工作区画像"),
            Message::system_execution_constraint_hint("执行约束临时 system"),
        ];
        strip_persist_injections(&mut messages);
        let names: Vec<Option<String>> = messages.iter().map(|m| m.name.clone()).collect();
        assert_eq!(
            names,
            vec![
                None,
                Some(CRABMATE_CONTEXT_SUMMARY_NAME.to_string()),
                Some(CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME.to_string()),
            ]
        );
    }

    /// 中断回落时 `messages` 尾部可能残留注入的 `role==user` 消息（编排 / LTM / 变更集）；
    /// 可见性判定必须在 strip **之后**进行，否则 `rposition(role=="user")` 的锚点会落到尾部注入上，
    /// 令本轮真正的助手部分输出被判为「非本轮」而静默跳过落盘。
    #[test]
    fn visibility_check_after_strip_keeps_interrupted_partial_output() {
        let mut messages = vec![
            Message::user_only("真实问题"),
            Message::assistant_only("被中断前的部分输出"),
            Message::user_server_injection(CRABMATE_LONG_TERM_MEMORY_NAME, "尾部注入"),
        ];
        // strip 前：锚点被尾部注入顶走，助手输出被判为非本轮。
        assert!(
            !current_turn_has_visible_assistant_output(&messages),
            "strip 前应因注入锚点错位而判定为无可见输出"
        );
        strip_persist_injections(&mut messages);
        assert!(
            current_turn_has_visible_assistant_output(&messages),
            "strip 后应识别到本轮被中断的部分助手输出"
        );
    }

    /// 取消常停在 `assistant(tool_calls=[半截 arguments])` 且本轮没有正文：结构修复会丢弃该残片，
    /// 使助手变成空消息。可见性必须在修复**之前**取快照，否则这一轮（连同本轮 user 消息）
    /// 会被整体跳过落盘，用户在服务端历史里丢失自己的输入。
    #[test]
    fn visibility_snapshot_before_tool_call_repair_keeps_cancelled_turn() {
        let mut messages = vec![
            Message::user_only("继续"),
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_truncated".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "run_command".to_string(),
                        arguments: "{\"command\":\"sleep\"".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
        ];
        assert!(
            current_turn_has_visible_assistant_output(&messages),
            "半截 tool_calls 在修复前应算作可见输出"
        );
        let repaired =
            crate::cm_agent::message_pipeline::complete_pending_tool_responses_for_persistence(
                &mut messages,
            );
        assert_eq!(repaired, 0, "非法 arguments 的残片应被丢弃而不是补占位");
        assert!(messages[1].tool_calls.is_none(), "残片应已丢弃");
        assert!(
            !current_turn_has_visible_assistant_output(&messages),
            "修复后助手为空；因此调用方必须使用修复前的快照判定可见性"
        );
    }
}
