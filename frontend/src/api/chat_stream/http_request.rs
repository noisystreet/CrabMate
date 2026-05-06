use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

use crabmate_sse_protocol::SSE_PROTOCOL_VERSION;

use crate::i18n::Locale;

use super::super::browser;
use super::super::client_llm_storage::{
    chat_temperature_override_from_storage, client_llm_json_for_chat_body,
    execution_mode_for_chat_body, executor_llm_json_for_chat_body,
    readonly_tool_ttl_cache_secs_for_chat_body,
};
use super::ChatStreamCallbacks;

/// `build_chat_stream_post_body` 的入参（避免超长形参列表）。
pub(super) struct ChatStreamPostBodyParts<'a> {
    pub(super) message: &'a str,
    pub(super) image_urls: &'a [String],
    pub(super) conversation_id: &'a Option<String>,
    pub(super) agent_role: &'a Option<String>,
    pub(super) approval_session_id: &'a Option<String>,
    pub(super) stream_resume_job_id: Option<u64>,
    pub(super) last_event_id: u64,
    pub(super) clarify_questionnaire_answers: &'a Option<serde_json::Value>,
}

pub(super) fn build_chat_stream_post_body(
    p: ChatStreamPostBodyParts<'_>,
) -> Result<serde_json::Value, String> {
    let ChatStreamPostBodyParts {
        message,
        image_urls,
        conversation_id,
        agent_role,
        approval_session_id,
        stream_resume_job_id,
        last_event_id,
        clarify_questionnaire_answers,
    } = p;
    let mut body = serde_json::json!({
        "message": message,
        "conversation_id": conversation_id,
        "agent_role": agent_role,
        "approval_session_id": approval_session_id,
        "client_sse_protocol": SSE_PROTOCOL_VERSION,
    });
    if !image_urls.is_empty() {
        body["image_urls"] = serde_json::json!(image_urls);
    }
    if let Some(cq) = clarify_questionnaire_answers {
        body["clarify_questionnaire_answers"] = cq.clone();
    }
    if let Some(jid) = stream_resume_job_id {
        body["stream_resume"] = serde_json::json!({
            "job_id": jid,
            "after_seq": last_event_id,
        });
    }
    if let Some(cl) = client_llm_json_for_chat_body() {
        body["client_llm"] = cl;
    }
    if let Some(el) = executor_llm_json_for_chat_body() {
        body["executor_llm"] = el;
    }
    if let Some(temp) = chat_temperature_override_from_storage() {
        body["temperature"] = serde_json::json!(temp);
    }
    if let Some(mode) = execution_mode_for_chat_body() {
        body["execution_mode"] = serde_json::json!(mode);
    }
    if let Some(secs) = readonly_tool_ttl_cache_secs_for_chat_body() {
        body["readonly_tool_ttl_cache_secs"] = serde_json::json!(secs);
    }
    Ok(body)
}

pub(super) fn build_chat_stream_fetch_request(
    body_json: &str,
    signal: &web_sys::AbortSignal,
    last_event_id: u64,
) -> Result<Request, String> {
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_mode(RequestMode::Cors);
    init.set_signal(Some(signal));
    let h = browser::auth_headers();
    let _ = h.set("Content-Type", "application/json");
    if last_event_id > 0 {
        let _ = h.set("Last-Event-ID", &last_event_id.to_string());
    }
    init.set_headers(&h);
    init.set_body(&wasm_bindgen::JsValue::from_str(body_json));
    Request::new_with_str_and_init("/chat/stream", &init).map_err(|e| format!("req: {:?}", e))
}

pub(super) fn apply_chat_stream_response_headers(
    resp: &Response,
    cbs: &ChatStreamCallbacks,
    stream_resume_job_id: &mut Option<u64>,
) {
    if let Some(cid) = resp.headers().get("x-conversation-id").ok().flatten() {
        let t = cid.trim();
        if !t.is_empty() {
            (cbs.on_conversation_id)(t.to_string());
        }
    }
    if let Some(jh) = resp.headers().get("x-stream-job-id").ok().flatten() {
        if let Ok(jid) = jh.trim().parse::<u64>() {
            *stream_resume_job_id = Some(jid);
            (cbs.on_stream_job_id)(jid);
        }
    }
}

pub(super) async fn chat_stream_read_error_body(
    resp: &Response,
    loc: Locale,
) -> Result<String, String> {
    let text_promise = resp.text().map_err(|e| format!("text: {:?}", e))?;
    Ok(JsFuture::from(text_promise)
        .await
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| crate::i18n::api_err_request_failed(loc).to_string()))
}

pub(super) async fn sleep_chat_stream_retry_backoff(attempt: u32) {
    let ms = (200u64).saturating_mul(1u64 << attempt.min(5));
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
}
