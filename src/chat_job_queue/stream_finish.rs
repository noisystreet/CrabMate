//! 流式任务在 `run_agent_turn` 之后的收尾：落盘、SSE 终端帧、`final_response` 兜底等。

use std::sync::Arc;

use crabmate_sse_protocol::StreamEndReason;
use log::{debug, error, info};
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};

use crate::AppState;
use crate::agent::agent_turn::AgentTurnJobOutcomeKind;
use crate::agent_role_turn::persisted_agent_role_after_turn;
use crate::config::AgentConfig;
use crate::sse::SseStreamHub;
use crate::types::{Message, message_content_as_str};

use super::WebChatQueueDeps;

/// Web 队列：`run_agent_turn` 成功后的 LTM 异步索引、剥离注入与会话按 revision 落盘。
pub(super) async fn post_turn_web_prepare_and_save(
    app: &AppState,
    cfg_snap: &Arc<AgentConfig>,
    conversation_id: &str,
    messages: &mut Vec<Message>,
    expected_revision: Option<u64>,
    request_agent_role: Option<&str>,
    persisted_active_agent_role: Option<&str>,
) -> crate::SaveConversationOutcome {
    let scope = conversation_id.to_string();
    let to_index = messages.clone();
    if let (Some(ltm), true) = (
        app.aux.long_term_memory.as_ref(),
        cfg_snap.long_term_memory.long_term_memory_enabled,
    ) {
        ltm.clone()
            .spawn_turn_memory_postprocess(Arc::clone(cfg_snap), scope, to_index);
    }
    crate::memory::long_term_memory::strip_long_term_memory_injections(messages);
    crate::workspace::changelist::strip_workspace_changelist_injections(messages);
    crate::types::strip_orchestration_injected_users_for_conversation_store(messages);
    let active_save =
        persisted_agent_role_after_turn(persisted_active_agent_role, request_agent_role);
    app.save_conversation_messages_if_revision(
        conversation_id.to_string(),
        messages.clone(),
        active_save.as_deref(),
        expected_revision,
    )
    .await
}

/// 流任务被取消且 **mpsc 仍有接收端** 时补发一条带 `code: STREAM_CANCELLED` 的控制面，便于前端与代理统一收尾（接收端已 drop 时仅 debug，避免误报）。
pub(crate) async fn emit_stream_cancelled_terminal(sse_tx: &mpsc::Sender<String>, job_id: u64) {
    if sse_tx.is_closed() {
        debug!(
            target: "crabmate",
            "stream 任务已取消且 SSE 已无接收端，跳过 STREAM_CANCELLED 帧 job_id={}",
            job_id
        );
        return;
    }
    let line =
        crate::sse::encode_message(crate::sse::SsePayload::Error(crate::sse::SseErrorBody {
            error: "流已取消".to_string(),
            code: Some(crate::types::SSE_STREAM_CANCELLED_CODE.to_string()),
            reason_code: None,
            turn_id: Some(job_id),
            sub_phase: None,
        }));
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

fn last_assistant_text_for_fallback(messages: &[Message]) -> Option<String> {
    let range_start = messages
        .iter()
        .rposition(|m| m.role == "user")
        .map(|idx| idx.saturating_add(1))
        .unwrap_or(0);
    messages
        .iter()
        .skip(range_start)
        .rev()
        .find(|m| m.role == "assistant")
        .and_then(|m| message_content_as_str(&m.content))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn current_turn_has_visible_assistant_output(messages: &[Message]) -> bool {
    let range_start = messages
        .iter()
        .rposition(|m| m.role == "user")
        .map(|idx| idx.saturating_add(1))
        .unwrap_or(0);
    messages.iter().skip(range_start).any(|m| {
        if m.role != "assistant" {
            return false;
        }
        let text_visible = message_content_as_str(&m.content)
            .map(str::trim)
            .is_some_and(|s| !s.is_empty());
        let reasoning_visible = m
            .reasoning_content
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty());
        text_visible || reasoning_visible
    })
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
    pub(super) r: Result<(), crate::agent::agent_turn::RunAgentTurnError>,
    pub(super) cancelled_by_signal: bool,
    pub(super) queue_deps: &'a WebChatQueueDeps,
    pub(super) sse_tx: &'a mpsc::Sender<String>,
    pub(super) job_id: u64,
    pub(super) messages: &'a mut Vec<Message>,
    pub(super) cfg_snap: &'a Arc<AgentConfig>,
    pub(super) app: &'a AppState,
    pub(super) conversation_id: &'a str,
    pub(super) expected_revision: Option<u64>,
    pub(super) request_agent_role: Option<&'a str>,
    pub(super) persisted_active_agent_role: Option<&'a str>,
    pub(super) stream_ended_sent: &'a mut bool,
}

pub(crate) async fn stream_job_outcome_after_agent_turn(
    ctx: StreamJobOutcomeCtx<'_>,
) -> (bool, bool, Option<String>, StreamEndReason) {
    let StreamJobOutcomeCtx {
        r,
        cancelled_by_signal,
        queue_deps,
        sse_tx,
        job_id,
        messages,
        cfg_snap,
        app,
        conversation_id,
        expected_revision,
        request_agent_role,
        persisted_active_agent_role,
        stream_ended_sent,
    } = ctx;
    match r {
        Ok(()) if cancelled_by_signal => {
            info!(target: "crabmate", "chat stream 任务已取消 job_id={}", job_id);
            (false, true, None, StreamEndReason::Cancelled)
        }
        Ok(()) => {
            let fallback_emitted = emit_missing_final_response_fallback_if_needed(
                queue_deps.sse_stream_hub.as_ref(),
                sse_tx,
                job_id,
                messages,
            )
            .await;
            let has_visible_output = current_turn_has_visible_assistant_output(messages);
            let end_reason = if fallback_emitted {
                StreamEndReason::Fallback
            } else if has_visible_output {
                StreamEndReason::Completed
            } else {
                StreamEndReason::NoOutput
            };
            let tiktoken_prompt_tokens =
                crate::agent::tiktoken_prompt_tokens::prompt_token_count_vendor_shaped_for_session(
                    cfg_snap, messages,
                );
            // 发送状态快照，包含完整消息列表
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
            // 先发 stream_ended 解除前端 busy，再做可能耗时的落盘/revision 同步，
            // 避免后处理阶段卡住导致 Web 长时间停在“模型生成中”。
            emit_stream_ended_once(
                sse_tx,
                job_id,
                end_reason,
                stream_ended_sent,
                "chat_job_queue::stream stream_ended_early",
                tiktoken_prompt_tokens.clone(),
            )
            .await;
            match post_turn_web_prepare_and_save(
                app,
                cfg_snap,
                conversation_id,
                messages,
                expected_revision,
                request_agent_role,
                persisted_active_agent_role,
            )
            .await
            {
                crate::SaveConversationOutcome::Saved => {
                    if let Some(new_rev) = app
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
                    (true, false, None, end_reason)
                }
                crate::SaveConversationOutcome::Conflict => {
                    let err_line = crate::conversation_conflict_sse_line();
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
                    (false, true, None, StreamEndReason::Cancelled)
                }
                AgentTurnJobOutcomeKind::FailureEmitSseError => {
                    error!(
                        target: "crabmate",
                        "chat stream 任务失败 job_id={} err_kind=agent_turn {}",
                        job_id,
                        e.diag_log_kv(),
                    );
                    let err_body = e.sse_error_payload(Some(job_id));
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
