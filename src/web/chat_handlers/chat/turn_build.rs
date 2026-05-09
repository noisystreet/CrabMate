//! 首轮/续轮消息组装、SSE 协议与流式请求解析（与 `chat` handler 共用的准备逻辑）。

use std::sync::Arc;

use axum::Json;
use axum::http::StatusCode;

use super::super::super::app_state::{AppState, ConversationTurnSeed};
use super::super::parse::{
    normalize_agent_role, normalize_chat_image_urls, normalize_client_conversation_id,
    parse_client_llm_override, parse_execution_mode_override, parse_executor_llm_override,
    parse_optional_chat_temperature, parse_readonly_tool_ttl_cache_secs,
    parse_seed_override_from_body,
};
use crate::agent_role_turn::maybe_apply_mid_session_agent_role_switch;
use crate::chat_job_queue;
use crate::clarification_questionnaire::normalize_clarify_questionnaire_answers_raw;
use crate::context_bootstrap::conversation_turn_bootstrap::{
    compose_new_conversation_messages, first_turn_project_context_user_message_for_web,
};
use crate::memory::agent_memory::load_memory_snippet;
use crate::types::{Message, message_user_with_images};
use crate::web::http_types::chat::{ApiError, ChatRequestBody, StreamResumeBody};
use crate::web::http_types::validation::validate_chat_request_payload_limits;

use super::builtin_skills::merge_system_prompt_with_workspace_skills_for_web;

pub(super) fn reject_if_client_sse_protocol_invalid(
    client_sse_protocol: Option<u8>,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    let Some(v) = client_sse_protocol else {
        return Ok(());
    };
    if v == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_SSE_CLIENT_PROTOCOL",
                message: "client_sse_protocol 非法（须为 1～255）".to_string(),
                reason_code: None,
            }),
        ));
    }
    if v > crate::sse::protocol::SSE_PROTOCOL_VERSION {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "SSE_CLIENT_TOO_NEW",
                message: "客户端声明的 SSE 协议版本高于服务端，请升级服务器或更换匹配的前端构建"
                    .to_string(),
                reason_code: None,
            }),
        ));
    }
    Ok(())
}

pub(super) fn parse_last_event_id(headers: &axum::http::HeaderMap) -> Option<u64> {
    let raw = headers.get(axum::http::HeaderName::from_static("last-event-id"))?;
    let s = raw.to_str().ok()?.trim();
    if s.is_empty() {
        return None;
    }
    s.parse::<u64>().ok()
}

/// `chat_stream_handler` 前半段：校验并解析请求体（降低主 handler 圈复杂度）。
pub(super) struct ChatStreamRequestParsed {
    pub(super) resume: Option<StreamResumeBody>,
    pub(super) image_urls: Vec<String>,
    pub(super) clarify: Option<crate::clarification_questionnaire::ClarifyAnswersNormalized>,
    pub(super) user_trim: String,
    pub(super) conversation_id: String,
    pub(super) agent_role: Option<String>,
    pub(super) temperature_override: Option<f32>,
    pub(super) seed_override: crate::types::LlmSeedOverride,
    pub(super) llm_override: Option<chat_job_queue::WebChatLlmOverride>,
    pub(super) executor_llm_override: Option<chat_job_queue::WebChatLlmOverride>,
    pub(super) execution_mode_override: Option<chat_job_queue::WebExecutionModeOverride>,
    pub(super) readonly_tool_ttl_cache_secs: Option<u64>,
}

pub(super) fn parse_chat_stream_request(
    state: &Arc<AppState>,
    body: &ChatRequestBody,
) -> Result<ChatStreamRequestParsed, (StatusCode, Json<ApiError>)> {
    validate_chat_request_payload_limits(body)?;
    let resume = body.stream_resume.clone();
    let image_urls = normalize_chat_image_urls(&body.image_urls).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_IMAGE_URLS",
                message: e,
                reason_code: None,
            }),
        )
    })?;
    let clarify = if let Some(ref c) = body.clarify_questionnaire_answers {
        normalize_clarify_questionnaire_answers_raw(c.questionnaire_id.clone(), c.answers.clone())
            .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_CLARIFY_QUESTIONNAIRE_ANSWERS",
                    message: e,
                    reason_code: None,
                }),
            )
        })?
    } else {
        None
    };
    let user_trim = body.message.trim().to_string();
    if user_trim.is_empty() && resume.is_none() && image_urls.is_empty() && clarify.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "EMPTY_MESSAGE",
                message: "提问内容不能为空（若仅发图须至少附带一张图片；澄清问卷作答可单独提交）"
                    .to_string(),
                reason_code: None,
            }),
        ));
    }
    reject_if_client_sse_protocol_invalid(body.client_sse_protocol)?;
    let conversation_id = normalize_client_conversation_id(body.conversation_id.as_deref())
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_CONVERSATION_ID",
                    message: e,
                    reason_code: None,
                }),
            )
        })?
        .unwrap_or_else(|| state.next_conversation_id());
    let agent_role = normalize_agent_role(body.agent_role.as_deref()).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_AGENT_ROLE",
                message: e,
                reason_code: None,
            }),
        )
    })?;
    let temperature_override = parse_optional_chat_temperature(body.temperature).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_TEMPERATURE",
                message: e,
                reason_code: None,
            }),
        )
    })?;
    let seed_override = parse_seed_override_from_body(body.seed, body.seed_policy.clone())
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_SEED",
                    message: e,
                    reason_code: None,
                }),
            )
        })?;
    let llm_override = parse_client_llm_override(body.client_llm.clone()).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_CLIENT_LLM",
                message: e,
                reason_code: None,
            }),
        )
    })?;
    let executor_llm_override =
        parse_executor_llm_override(body.executor_llm.clone()).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_EXECUTOR_LLM",
                    message: e,
                    reason_code: None,
                }),
            )
        })?;
    let execution_mode_override = parse_execution_mode_override(body.execution_mode.clone())
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_EXECUTION_MODE",
                    message: e,
                    reason_code: None,
                }),
            )
        })?;
    let readonly_tool_ttl_cache_secs =
        parse_readonly_tool_ttl_cache_secs(body.readonly_tool_ttl_cache_secs).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_READONLY_TOOL_TTL_CACHE_SECS",
                    message: e,
                    reason_code: None,
                }),
            )
        })?;
    Ok(ChatStreamRequestParsed {
        resume,
        image_urls,
        clarify,
        user_trim,
        conversation_id,
        agent_role,
        temperature_override,
        seed_override,
        llm_override,
        executor_llm_override,
        execution_mode_override,
        readonly_tool_ttl_cache_secs,
    })
}

pub(super) async fn build_messages_for_turn(
    state: &Arc<AppState>,
    conversation_id: &str,
    user_msg: &str,
    image_urls: &[String],
    agent_role: Option<&str>,
) -> Result<ConversationTurnSeed, String> {
    let root_str = state.effective_workspace_path().await;
    let workspace_is_set = state.workspace_is_set().await;
    let root = std::path::PathBuf::from(root_str);
    let last_user = if image_urls.is_empty() {
        Message::user_only(user_msg.to_string())
    } else {
        message_user_with_images(user_msg, image_urls)
    };
    if let Some(mut seed) = state.load_conversation_seed(conversation_id).await {
        let persisted = seed.persisted_active_agent_role.clone();
        {
            let cfg = state.http.cfg.read().await;
            if let Some(id) = agent_role.map(str::trim).filter(|s| !s.is_empty()) {
                cfg.system_prompt_for_new_conversation(Some(id))
                    .map_err(|e| e.to_string())?;
            }
            maybe_apply_mid_session_agent_role_switch(
                &cfg,
                &mut seed.messages,
                persisted.as_deref(),
                agent_role,
                &state.aux.process_handles.tool_outcome_recorder,
            )?;
            let role_for_turn = agent_role
                .and_then(|s| {
                    let t = s.trim();
                    if t.is_empty() { None } else { Some(t) }
                })
                .or(persisted.as_deref());
            let system_for_turn = cfg
                .system_prompt_for_new_conversation(role_for_turn)
                .map_err(|e| e.to_string())?
                .to_string();
            let system_for_turn = state
                .aux
                .process_handles
                .tool_outcome_recorder
                .augment_system_prompt(&system_for_turn, &cfg);
            let system_for_turn = if workspace_is_set {
                merge_system_prompt_with_workspace_skills_for_web(
                    system_for_turn,
                    cfg.skills.skills_enabled,
                    cfg.skills.skills_dir.as_str(),
                    cfg.skills.skills_max_chars,
                    cfg.skills.skills_top_k,
                    root.as_path(),
                    user_msg,
                )
            } else {
                system_for_turn
            };
            if let Some(first) = seed.messages.first_mut()
                && first.role == "system"
            {
                first.content = Some(crate::types::MessageContent::Text(system_for_turn));
            }
        }
        seed.messages.push(last_user);
        return Ok(seed);
    }
    let cfg = state.http.cfg.read().await;
    let system_for_turn = cfg
        .system_prompt_for_new_conversation(agent_role)
        .map_err(|e| e.to_string())?
        .to_string();
    let system_for_turn = state
        .aux
        .process_handles
        .tool_outcome_recorder
        .augment_system_prompt(&system_for_turn, &cfg);
    let system_for_turn = if workspace_is_set {
        merge_system_prompt_with_workspace_skills_for_web(
            system_for_turn,
            cfg.skills.skills_enabled,
            cfg.skills.skills_dir.as_str(),
            cfg.skills.skills_max_chars,
            cfg.skills.skills_top_k,
            root.as_path(),
            user_msg,
        )
    } else {
        system_for_turn
    };
    let memory_snippet =
        if workspace_is_set && cfg.context_bootstrap_inject.agent_memory_file_enabled {
            load_memory_snippet(
                &root,
                cfg.context_bootstrap_inject.agent_memory_file.as_str(),
                cfg.context_bootstrap_inject.agent_memory_file_max_chars,
            )
        } else {
            None
        };

    let combined = first_turn_project_context_user_message_for_web(
        workspace_is_set,
        root.as_path(),
        &cfg,
        memory_snippet,
    )
    .await;
    let messages = compose_new_conversation_messages(&system_for_turn, combined, Some(last_user));
    Ok(ConversationTurnSeed {
        messages,
        expected_revision: None,
        persisted_active_agent_role: None,
    })
}
