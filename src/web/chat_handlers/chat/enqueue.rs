//! `prepare_json_chat_enqueue`、请求解析与同步 JSON 队列入队（`POST /chat` 共用）。

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::Json;
use axum::http::{HeaderMap, StatusCode};
use log::{debug, error, info};

use super::super::conflict::conversation_conflict_api_error;
use super::super::parse::{
    ensure_bearer_api_key_for_chat, normalize_agent_role, normalize_chat_image_urls,
    normalize_client_conversation_id, parse_client_llm_override, parse_executor_llm_override,
    parse_optional_chat_temperature, parse_readonly_tool_ttl_cache_secs,
    parse_seed_override_from_body,
};
use crate::chat_job_queue;
use crate::clarification_questionnaire::{
    ClarifyAnswersNormalized, merge_user_text_with_clarification_answers,
    normalize_clarify_questionnaire_answers_raw,
};
use crate::redact;
use crate::types::Message;
use crate::user_message_file_refs::{
    expand_at_file_refs_in_user_message, user_message_has_workspace_file_ref_syntax,
};
use crate::web::app_state::ConversationTurnSeed;
use crate::web::app_state_facets::WebChatTurnAppFacet;
use crate::web::audit;
use crate::web::http_types::chat::{ApiError, ChatRequestBody};
use crate::web::http_types::validation::validate_chat_request_payload_limits;

use super::turn_build::{
    bad_request, build_messages_for_turn, reject_if_client_sse_protocol_invalid,
};

/// 与 `chat_handler` 共用的 JSON 入队：解析 `@`、组装首轮消息、选定工作目录。
pub(crate) struct PreparedJsonChatEnqueue {
    pub(crate) conversation_id: String,
    pub(crate) turn_seed: ConversationTurnSeed,
    pub(crate) work_dir: PathBuf,
    pub(crate) workspace_is_set: bool,
    pub(crate) msg_for_log: String,
}

pub(crate) async fn prepare_json_chat_enqueue(
    state: &WebChatTurnAppFacet,
    user_trim: &str,
    clarify: Option<ClarifyAnswersNormalized>,
    image_urls: &[String],
    agent_role: Option<String>,
    session_mode: Option<String>,
    conversation_id: String,
) -> Result<PreparedJsonChatEnqueue, (StatusCode, Json<ApiError>)> {
    let eff_ws_raw = state.effective_workspace_path().await;
    let eff_ws = eff_ws_raw.trim().to_string();
    if eff_ws.is_empty() && user_message_has_workspace_file_ref_syntax(user_trim) {
        return Err(bad_request(
            "WORKSPACE_NOT_SET",
            "未设置工作区：无法在消息中使用 `file:///` / `@` 引用工作区内文件。请先在侧栏工作区面板选择或提交目录。",
        ));
    }
    let work_dir_for_expand = std::path::PathBuf::from(eff_ws_raw.clone());
    let msg = {
        let cfg = state.cfg.read().await;
        expand_at_file_refs_in_user_message(user_trim, work_dir_for_expand.as_path(), &cfg)
            .map_err(|e| bad_request("INVALID_AT_FILE_REF", e))?
    };
    let msg = merge_user_text_with_clarification_answers(msg, clarify);
    let turn_seed = build_messages_for_turn(
        state,
        &conversation_id,
        &msg,
        image_urls,
        agent_role.as_deref(),
        session_mode.as_deref(),
    )
    .await
    .map_err(|e| {
        if let Some(msg) = crate::config::skills_slash::SkillSlashError::strip_turn_err(&e) {
            bad_request("SKILL_INVOKE_FAILED", msg)
        } else if e.contains("session_mode") {
            bad_request("INVALID_SESSION_MODE", e)
        } else {
            bad_request("INVALID_AGENT_ROLE", e)
        }
    })?;
    let workspace_is_set = state.workspace_is_set().await;
    let work_dir_for_job = if eff_ws.is_empty() {
        let cfg = state.cfg.read().await;
        std::path::PathBuf::from(cfg.command_exec.run_command_working_dir.clone())
    } else {
        std::path::PathBuf::from(eff_ws.clone())
    };
    Ok(PreparedJsonChatEnqueue {
        conversation_id,
        turn_seed,
        work_dir: work_dir_for_job,
        workspace_is_set,
        msg_for_log: msg,
    })
}

/// `POST /chat` / **`POST /chat/async`** 共用：校验请求体（**不含** `prepare_json_chat_enqueue` 与内置命令）。
pub(crate) struct ParsedChatRequestForEnqueue {
    pub(crate) image_urls: Vec<String>,
    pub(crate) clarify: Option<ClarifyAnswersNormalized>,
    pub(crate) user_trim: String,
    pub(crate) conversation_id: String,
    pub(crate) agent_role: Option<String>,
    pub(crate) session_mode: Option<String>,
    pub(crate) temperature_override: Option<f32>,
    pub(crate) seed_override: crate::types::LlmSeedOverride,
    pub(crate) client_sse_protocol: Option<u8>,
    pub(crate) llm_override: Option<chat_job_queue::WebChatLlmOverride>,
    pub(crate) executor_llm_override: Option<chat_job_queue::WebChatLlmOverride>,
    pub(crate) readonly_tool_ttl_cache_secs: Option<u64>,
}

type NormalizedChatEnqueuePayload = (Vec<String>, Option<ClarifyAnswersNormalized>, String);

fn normalize_chat_enqueue_payload(
    body: &ChatRequestBody,
) -> Result<NormalizedChatEnqueuePayload, (StatusCode, Json<ApiError>)> {
    validate_chat_request_payload_limits(body)?;
    let image_urls = normalize_chat_image_urls(&body.image_urls)
        .map_err(|e| bad_request("INVALID_IMAGE_URLS", e))?;
    let clarify = if let Some(ref c) = body.clarify_questionnaire_answers {
        normalize_clarify_questionnaire_answers_raw(c.questionnaire_id.clone(), c.answers.clone())
            .map_err(|e| bad_request("INVALID_CLARIFY_QUESTIONNAIRE_ANSWERS", e))?
    } else {
        None
    };
    let user_trim = body.message.trim();
    if user_trim.is_empty() && image_urls.is_empty() && clarify.is_none() {
        return Err(bad_request(
            "EMPTY_MESSAGE",
            "提问内容不能为空（若仅发图须至少附带一张图片；澄清问卷作答可单独提交）",
        ));
    }
    reject_if_client_sse_protocol_invalid(body.client_sse_protocol)?;
    Ok((image_urls, clarify, user_trim.to_string()))
}

pub(crate) async fn parse_chat_request_for_enqueue(
    state: &WebChatTurnAppFacet,
    body: &ChatRequestBody,
) -> Result<ParsedChatRequestForEnqueue, (StatusCode, Json<ApiError>)> {
    let (image_urls, clarify, user_trim) = normalize_chat_enqueue_payload(body)?;
    let parsed =
        parse_chat_request_for_enqueue_tail(state, body, image_urls, clarify, user_trim.as_str())?;
    ensure_bearer_api_key_for_chat(state, &parsed.llm_override).await?;
    Ok(parsed)
}

fn parse_chat_request_for_enqueue_tail(
    state: &WebChatTurnAppFacet,
    body: &ChatRequestBody,
    image_urls: Vec<String>,
    clarify: Option<ClarifyAnswersNormalized>,
    user_trim: &str,
) -> Result<ParsedChatRequestForEnqueue, (StatusCode, Json<ApiError>)> {
    let conversation_id = normalize_client_conversation_id(body.conversation_id.as_deref())
        .map_err(|e| bad_request("INVALID_CONVERSATION_ID", e))?
        .unwrap_or_else(|| state.next_conversation_id());
    let agent_role = normalize_agent_role(body.agent_role.as_deref())
        .map_err(|e| bad_request("INVALID_AGENT_ROLE", e))?;
    let session_mode = crate::types::parse_optional_session_mode(body.session_mode.as_deref())
        .map_err(|e| bad_request("INVALID_SESSION_MODE", e))?
        .map(|m| m.as_str().to_string());
    let temperature_override = parse_optional_chat_temperature(body.temperature)
        .map_err(|e| bad_request("INVALID_TEMPERATURE", e))?;
    let seed_override = parse_seed_override_from_body(body.seed, body.seed_policy.clone())
        .map_err(|e| bad_request("INVALID_SEED", e))?;
    let llm_override = parse_client_llm_override(crate::user_data::merge_client_llm_body(
        body.client_llm.clone(),
    ))
    .map_err(|e| bad_request("INVALID_CLIENT_LLM", e))?;
    let executor_llm_override = parse_executor_llm_override(
        crate::user_data::merge_executor_llm_body(body.executor_llm.clone()),
    )
    .map_err(|e| bad_request("INVALID_EXECUTOR_LLM", e))?;
    let readonly_tool_ttl_cache_secs =
        parse_readonly_tool_ttl_cache_secs(body.readonly_tool_ttl_cache_secs)
            .map_err(|e| bad_request("INVALID_READONLY_TOOL_TTL_CACHE_SECS", e))?;
    Ok(ParsedChatRequestForEnqueue {
        image_urls,
        clarify,
        user_trim: user_trim.to_string(),
        conversation_id,
        agent_role,
        session_mode,
        temperature_override,
        seed_override,
        client_sse_protocol: body.client_sse_protocol,
        llm_override,
        executor_llm_override,
        readonly_tool_ttl_cache_secs,
    })
}

pub(crate) async fn web_request_audit_for_turn(
    state: &WebChatTurnAppFacet,
    headers: &HeaderMap,
    peer: SocketAddr,
) -> crate::WebRequestAudit {
    let cfg = state.cfg.read().await;
    audit::web_request_audit_from_http(&cfg, headers, peer)
}

/// 组装 JSON chat 队列 envelope（`POST /chat` 与 `/chat/async` 共用）。
pub(crate) fn json_chat_job_envelope(
    state: &WebChatTurnAppFacet,
    job_id: u64,
    prepared: PreparedJsonChatEnqueue,
    parsed: &ParsedChatRequestForEnqueue,
    request_audit: crate::WebRequestAudit,
    headers: &HeaderMap,
) -> chat_job_queue::WebChatJobEnvelope {
    chat_job_queue::WebChatJobEnvelope {
        job_id,
        queue_deps: state.chat.chat_queue_job_deps.clone(),
        app: state.chat_job_app_facet(),
        conversation_id: prepared.conversation_id,
        messages: prepared.turn_seed.messages,
        expected_revision: prepared.turn_seed.expected_revision,
        request_agent_role: parsed.agent_role.clone(),
        persisted_active_agent_role: prepared.turn_seed.persisted_active_agent_role.clone(),
        request_session_mode: parsed.session_mode.clone(),
        persisted_active_session_mode: prepared.turn_seed.persisted_active_session_mode.clone(),
        work_dir: prepared.work_dir,
        workspace_is_set: prepared.workspace_is_set,
        temperature_override: parsed.temperature_override,
        seed_override: parsed.seed_override,
        client_sse_protocol: parsed.client_sse_protocol,
        llm_override: parsed.llm_override.clone(),
        executor_llm_override: parsed.executor_llm_override.clone(),
        readonly_tool_ttl_cache_secs: parsed.readonly_tool_ttl_cache_secs,
        request_audit,
        request_id: None,
        github_token: crate::web::github_token_request::extract_github_token_from_headers(headers),
    }
}

pub(crate) async fn enqueue_and_wait_json_chat(
    state: WebChatTurnAppFacet,
    peer: SocketAddr,
    headers: &HeaderMap,
    parsed: ParsedChatRequestForEnqueue,
) -> Result<(Vec<Message>, u64), (StatusCode, Json<ApiError>)> {
    let prepared = prepare_json_chat_enqueue(
        &state,
        parsed.user_trim.as_str(),
        parsed.clarify.clone(),
        &parsed.image_urls,
        parsed.agent_role.clone(),
        parsed.session_mode.clone(),
        parsed.conversation_id.clone(),
    )
    .await?;
    let job_id = state.chat.chat_queue.next_job_id();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    debug!(
        target: "crabmate",
        "chat json 请求摘要 job_id={} user_len={} user_preview={}",
        job_id,
        prepared.msg_for_log.len(),
        redact::preview_chars(&prepared.msg_for_log, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    info!(target: "crabmate", "chat json 任务入队 job_id={}", job_id);
    let request_audit = web_request_audit_for_turn(&state, headers, peer).await;
    let envelope =
        json_chat_job_envelope(&state, job_id, prepared, &parsed, request_audit, headers);
    state
        .chat
        .chat_queue
        .try_submit_json(chat_job_queue::JsonSubmitParams { envelope, reply_tx })
        .map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ApiError::new(
                    "QUEUE_FULL",
                    format!(
                        "对话任务队列已满（最多等待 {} 个），请稍后重试",
                        e.max_pending
                    ),
                )),
            )
        })?;
    let messages = reply_rx
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    "INTERNAL_ERROR",
                    "对话任务被取消或内部错误".to_string(),
                )),
            )
        })?
        .map_err(|e| match e {
            chat_job_queue::ChatJsonJobFailure::ConversationConflict => {
                conversation_conflict_api_error()
            }
            chat_job_queue::ChatJsonJobFailure::Agent(err) => {
                error!(
                    target: "crabmate",
                    "chat json 队列任务失败 job_id={} err_kind=agent_turn {}",
                    job_id,
                    err.diag_log_kv(),
                );
                let status = err.suggested_http_status();
                let body = err.http_api_error();
                (status, Json(body))
            }
        })?;
    Ok((messages, job_id))
}
