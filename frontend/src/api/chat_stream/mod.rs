//! `/chat/stream`：`fetch` + SSE 帧解析与 `sse_dispatch` 桥接。
//!
//! 子模块：[`http_request`]（POST 体与请求头）、[`body_reader`]（ReadableStream 消费）、[`sse_frame`]（SSE 块解析）。

mod body_reader;
mod http_request;
mod send_helpers;
mod sse_frame;

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::Response;

use crate::i18n::Locale;

use send_helpers::{
    ChatStreamRoundOutcome, chat_stream_fetch_retry_exhausted, run_chat_stream_http_round,
};

pub type OnToolCallFn = std::rc::Rc<
    dyn Fn(String, String, Option<String>, Option<String>, Option<String>, Option<String>),
>;

pub struct ChatStreamCallbacks {
    pub on_delta: std::rc::Rc<dyn Fn(String)>,
    pub on_done: std::rc::Rc<dyn Fn()>,
    pub on_error: std::rc::Rc<dyn Fn(String)>,
    pub on_workspace_changed: std::rc::Rc<dyn Fn()>,
    pub on_tool_status: std::rc::Rc<dyn Fn(bool)>,
    pub on_tool_result: std::rc::Rc<dyn Fn(crate::sse_dispatch::ToolResultInfo)>,
    pub on_approval: std::rc::Rc<dyn Fn(crate::sse_dispatch::CommandApprovalRequest)>,
    pub on_conversation_id: std::rc::Rc<dyn Fn(String)>,
    /// SSE `conversation_saved.revision`，供 `POST /chat/branch`。
    pub on_conversation_revision: std::rc::Rc<dyn Fn(u64)>,
    /// 收到 `stream_ended` 控制面时调用（如 `completed` / `cancelled` / `conflict` 等）。
    pub on_stream_ended: std::rc::Rc<dyn Fn(String)>,
    /// 响应头 **`x-stream-job-id`**（新流首包；用于断线重连）。
    pub on_stream_job_id: std::rc::Rc<dyn Fn(u64)>,
    /// 每条 SSE 事件的 **`id:`**（单调序号），供断线后 `stream_resume.after_seq` / `Last-Event-ID`。
    pub on_last_sse_event_id: std::rc::Rc<dyn Fn(u64)>,
    /// 控制面 `assistant_answer_phase`：后续 `on_delta` 为终答（此前为思维链）。
    pub on_assistant_answer_phase: std::rc::Rc<dyn Fn()>,
    pub on_staged_plan_step_started:
        std::rc::Rc<dyn Fn(crate::sse_dispatch::StagedPlanStepStartInfo)>,
    pub on_staged_plan_step_finished:
        std::rc::Rc<dyn Fn(crate::sse_dispatch::StagedPlanStepEndInfo)>,
    /// SSE `clarification_questionnaire`（模型经工具触发）。
    pub on_clarification_questionnaire:
        std::rc::Rc<dyn Fn(crate::sse_dispatch::ClarificationQuestionnaireInfo)>,
    pub on_thinking_trace: std::rc::Rc<dyn Fn(crate::sse_dispatch::ThinkingTraceInfo)>,
    pub on_timeline_log: std::rc::Rc<dyn Fn(crate::sse_dispatch::TimelineLogInfo)>,
    /// SSE `tool_call`：工具调用事件，包含名称、摘要、参数预览和完整参数。
    pub on_tool_call: OnToolCallFn,
}

impl Clone for ChatStreamCallbacks {
    fn clone(&self) -> Self {
        Self {
            on_delta: std::rc::Rc::clone(&self.on_delta),
            on_done: std::rc::Rc::clone(&self.on_done),
            on_error: std::rc::Rc::clone(&self.on_error),
            on_workspace_changed: std::rc::Rc::clone(&self.on_workspace_changed),
            on_tool_status: std::rc::Rc::clone(&self.on_tool_status),
            on_tool_result: std::rc::Rc::clone(&self.on_tool_result),
            on_approval: std::rc::Rc::clone(&self.on_approval),
            on_conversation_id: std::rc::Rc::clone(&self.on_conversation_id),
            on_conversation_revision: std::rc::Rc::clone(&self.on_conversation_revision),
            on_stream_ended: std::rc::Rc::clone(&self.on_stream_ended),
            on_stream_job_id: std::rc::Rc::clone(&self.on_stream_job_id),
            on_last_sse_event_id: std::rc::Rc::clone(&self.on_last_sse_event_id),
            on_assistant_answer_phase: std::rc::Rc::clone(&self.on_assistant_answer_phase),
            on_staged_plan_step_started: std::rc::Rc::clone(&self.on_staged_plan_step_started),
            on_staged_plan_step_finished: std::rc::Rc::clone(&self.on_staged_plan_step_finished),
            on_clarification_questionnaire: std::rc::Rc::clone(
                &self.on_clarification_questionnaire,
            ),
            on_thinking_trace: std::rc::Rc::clone(&self.on_thinking_trace),
            on_timeline_log: std::rc::Rc::clone(&self.on_timeline_log),
            on_tool_call: std::rc::Rc::clone(&self.on_tool_call),
        }
    }
}

/// `/chat/stream` 请求参数（缩短 [`send_chat_stream`] 形参列表）。
pub struct SendChatStreamParams<'a> {
    pub message: String,
    pub image_urls: Vec<String>,
    pub conversation_id: Option<String>,
    pub agent_role: Option<String>,
    pub approval_session_id: Option<String>,
    pub stream_resume_job_id: Option<u64>,
    pub stream_resume_after_seq: Option<u64>,
    pub signal: &'a web_sys::AbortSignal,
    pub cbs: ChatStreamCallbacks,
    pub loc: Locale,
    /// 可选：`POST /chat/stream` 的 `clarify_questionnaire_answers`（`questionnaire_id` + `answers`）。
    pub clarify_questionnaire_answers: Option<serde_json::Value>,
}

/// `/chat/stream`：支持 **`Last-Event-ID`** 与 JSON **`stream_resume`** 断线重连（网络抖动时自动重试若干次）。
pub async fn send_chat_stream(p: SendChatStreamParams<'_>) -> Result<(), String> {
    let SendChatStreamParams {
        message,
        image_urls,
        conversation_id,
        agent_role,
        approval_session_id,
        mut stream_resume_job_id,
        stream_resume_after_seq,
        signal,
        cbs,
        loc,
        clarify_questionnaire_answers,
    } = p;
    let w = super::browser::window().ok_or_else(|| "no window".to_string())?;
    let mut last_event_id: u64 = stream_resume_after_seq.unwrap_or(0);
    let mut attempt: u32 = 0;
    loop {
        if signal.aborted() {
            return Ok(());
        }
        let body =
            http_request::build_chat_stream_post_body(http_request::ChatStreamPostBodyParts {
                message: &message,
                image_urls: &image_urls,
                conversation_id: &conversation_id,
                agent_role: &agent_role,
                approval_session_id: &approval_session_id,
                stream_resume_job_id,
                last_event_id,
                clarify_questionnaire_answers: &clarify_questionnaire_answers,
            })?;
        let body_json = serde_json::to_string(&body).map_err(|e| e.to_string())?;
        let req = http_request::build_chat_stream_fetch_request(&body_json, signal, last_event_id)?;
        let resp_val = match JsFuture::from(w.fetch_with_request(&req)).await {
            Ok(v) => v,
            Err(e) => {
                if chat_stream_fetch_retry_exhausted(stream_resume_job_id, attempt) {
                    return Err(format!("fetch: {:?}", e));
                }
                attempt = attempt.saturating_add(1);
                http_request::sleep_chat_stream_retry_backoff(attempt).await;
                continue;
            }
        };
        let resp: Response = resp_val.dyn_into().map_err(|_| "not Response")?;
        match run_chat_stream_http_round(
            resp,
            &cbs,
            &mut stream_resume_job_id,
            signal,
            &mut last_event_id,
            loc,
        )
        .await?
        {
            ChatStreamRoundOutcome::Completed => return Ok(()),
            ChatStreamRoundOutcome::ResumeReconnect => {}
        }
        if stream_resume_job_id.is_none() {
            return Err(crate::i18n::api_err_no_response_body(loc).to_string());
        }
        attempt = attempt.saturating_add(1);
        if attempt >= 6 {
            return Err(crate::i18n::api_err_request_failed(loc).to_string());
        }
        http_request::sleep_chat_stream_retry_backoff(attempt).await;
    }
}
