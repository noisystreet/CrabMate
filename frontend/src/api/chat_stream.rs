//! `/chat/stream`：`fetch` + SSE 帧解析与 `sse_dispatch` 桥接。

use futures_util::future::{Either, select};
use gloo_timers::future::TimeoutFuture;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

use crabmate_sse_protocol::{
    SSE_PROTOCOL_VERSION, StreamEndReason, extract_stream_ended_reason, is_sse_done_sentinel,
    join_sse_data_lines, parse_sse_event_id,
};

use crate::i18n::Locale;
use crate::sse_dispatch::{
    CommandApprovalRequest, SseCallbacks, StagedPlanStepEndInfo, StagedPlanStepStartInfo,
    ThinkingTraceInfo, TimelineLogInfo, ToolResultInfo, try_dispatch_sse_control_payload,
};

use super::browser::{auth_headers, window};
use super::client_llm_storage::{
    chat_temperature_override_from_storage, client_llm_json_for_chat_body,
    execution_mode_for_chat_body, executor_llm_json_for_chat_body,
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
    pub on_tool_result: std::rc::Rc<dyn Fn(ToolResultInfo)>,
    pub on_approval: std::rc::Rc<dyn Fn(CommandApprovalRequest)>,
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
    pub on_staged_plan_step_started: std::rc::Rc<dyn Fn(StagedPlanStepStartInfo)>,
    pub on_staged_plan_step_finished: std::rc::Rc<dyn Fn(StagedPlanStepEndInfo)>,
    /// SSE `clarification_questionnaire`（模型经工具触发）。
    pub on_clarification_questionnaire:
        std::rc::Rc<dyn Fn(crate::sse_dispatch::ClarificationQuestionnaireInfo)>,
    pub on_thinking_trace: std::rc::Rc<dyn Fn(ThinkingTraceInfo)>,
    pub on_timeline_log: std::rc::Rc<dyn Fn(TimelineLogInfo)>,
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

/// 块边界可能截断 UTF-8：只把从开头起「完整码点」前缀解码进 `text`，余字节留在 `raw`。
fn append_chunk_to_text_buffer(raw: &mut Vec<u8>, chunk: &[u8], text: &mut String) {
    raw.extend_from_slice(chunk);
    loop {
        if raw.is_empty() {
            break;
        }
        match std::str::from_utf8(raw.as_slice()) {
            Ok(s) => {
                text.push_str(s);
                raw.clear();
                break;
            }
            Err(e) => {
                let n = e.valid_up_to();
                if n == 0 {
                    break;
                }
                text.push_str(std::str::from_utf8(&raw[..n]).expect("valid_up_to"));
                raw.drain(..n);
            }
        }
    }
}

fn build_chat_stream_post_body(
    message: &str,
    image_urls: &[String],
    conversation_id: &Option<String>,
    agent_role: &Option<String>,
    approval_session_id: &Option<String>,
    stream_resume_job_id: Option<u64>,
    last_event_id: u64,
    clarify_questionnaire_answers: &Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
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
    Ok(body)
}

fn build_chat_stream_fetch_request(
    body_json: &str,
    signal: &web_sys::AbortSignal,
    last_event_id: u64,
) -> Result<Request, String> {
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_mode(RequestMode::Cors);
    init.set_signal(Some(signal));
    let h = auth_headers();
    let _ = h.set("Content-Type", "application/json");
    if last_event_id > 0 {
        let _ = h.set("Last-Event-ID", &last_event_id.to_string());
    }
    init.set_headers(&h);
    init.set_body(&wasm_bindgen::JsValue::from_str(body_json));
    Request::new_with_str_and_init("/chat/stream", &init).map_err(|e| format!("req: {:?}", e))
}

fn apply_chat_stream_response_headers(
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

async fn chat_stream_read_error_body(resp: &Response, loc: Locale) -> Result<String, String> {
    let text_promise = resp.text().map_err(|e| format!("text: {:?}", e))?;
    Ok(JsFuture::from(text_promise)
        .await
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| crate::i18n::api_err_request_failed(loc).to_string()))
}

async fn sleep_chat_stream_retry_backoff(attempt: u32) {
    let ms = (200u64).saturating_mul(1u64 << attempt.min(5));
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
}

/// 已收到 `stream_ended` 后，部分浏览器/代理可能长期不结束 body；超时则 `releaseLock` 结束挂起。
const POST_STREAM_ENDED_READ_TIMEOUT_MS: u32 = 25_000;

/// 尚未收到 `stream_ended` 时，单次 `read()` 若长期无字节（断流、掉帧、代理挂起），会永远阻塞；设上限以便回落 busy。
/// 长思考无 SSE 的网关较少见；若仍误判可调大或做配置。
const PRE_STREAM_ENDED_READ_STALL_TIMEOUT_MS: u32 = 300_000;

/// 两次「含 `data:` 的有效负载」之间的最大间隔（毫秒）。代理可能周期性下发不含 `data:` 的注释帧，
/// 使 `read()` 频繁返回，从而永远不触发 [`PRE_STREAM_ENDED_READ_STALL_TIMEOUT_MS`]；此上限仍可结束悬挂流。
/// 断线重连路径亦依赖此项（该路径不设单次 read 超时）。
const SSE_MEANINGFUL_PAYLOAD_IDLE_TIMEOUT_MS: f64 = 180_000.0;

/// 消费 `/chat/stream` 响应体：UTF-8 重组、SSE 分帧与尾部 flush（与断线重连时的读失败语义一致）。
async fn consume_chat_stream_response_body(
    rb: web_sys::ReadableStream,
    signal: &web_sys::AbortSignal,
    last_event_id: &mut u64,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
    stream_resume_job_id: Option<u64>,
) -> Result<(bool, bool), String> {
    let reader: web_sys::ReadableStreamDefaultReader = rb
        .get_reader()
        .dyn_into()
        .map_err(|_| crate::i18n::api_err_stream_reader(loc).to_string())?;

    let mut raw: Vec<u8> = Vec::new();
    let mut buffer = String::new();
    let mut stream_finished_normally = false;
    let mut saw_stream_ended = false;
    let mut last_meaningful_payload_ms = js_sys::Date::now();
    loop {
        if signal.aborted() {
            return Ok((true, saw_stream_ended));
        }
        if !saw_stream_ended {
            let now = js_sys::Date::now();
            if now - last_meaningful_payload_ms > SSE_MEANINGFUL_PAYLOAD_IDLE_TIMEOUT_MS {
                reader.release_lock();
                stream_finished_normally = true;
                break;
            }
        }
        let chunk: wasm_bindgen::JsValue = if saw_stream_ended {
            match select(
                JsFuture::from(reader.read()),
                TimeoutFuture::new(POST_STREAM_ENDED_READ_TIMEOUT_MS),
            )
            .await
            {
                Either::Left((Ok(c), _)) => c,
                Either::Left((Err(e), _)) => {
                    if stream_resume_job_id.is_none() {
                        return Err(crate::i18n::api_err_stream_read(&e));
                    }
                    break;
                }
                Either::Right(((), _pending_read)) => {
                    reader.release_lock();
                    stream_finished_normally = true;
                    break;
                }
            }
        } else if stream_resume_job_id.is_none() {
            match select(
                JsFuture::from(reader.read()),
                TimeoutFuture::new(PRE_STREAM_ENDED_READ_STALL_TIMEOUT_MS),
            )
            .await
            {
                Either::Left((Ok(c), _)) => c,
                Either::Left((Err(e), _)) => {
                    return Err(crate::i18n::api_err_stream_read(&e));
                }
                Either::Right(((), _)) => {
                    reader.release_lock();
                    stream_finished_normally = true;
                    break;
                }
            }
        } else {
            match JsFuture::from(reader.read()).await {
                Ok(c) => c,
                Err(e) => {
                    if stream_resume_job_id.is_none() {
                        return Err(crate::i18n::api_err_stream_read(&e));
                    }
                    break;
                }
            }
        };
        let done = js_sys::Reflect::get(&chunk, &JsValue::from_str("done"))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if done {
            stream_finished_normally = true;
            break;
        }
        let value =
            js_sys::Reflect::get(&chunk, &JsValue::from_str("value")).unwrap_or(JsValue::NULL);
        if let Some(u8) = value.dyn_ref::<js_sys::Uint8Array>() {
            append_chunk_to_text_buffer(&mut raw, &u8.to_vec(), &mut buffer);
        }
        let meaningful =
            process_sse_buffer(&mut buffer, last_event_id, &mut saw_stream_ended, cbs, loc)?;
        if meaningful > 0 {
            last_meaningful_payload_ms = js_sys::Date::now();
        }
        // 不在此处因 `stream_ended` 提前 break：提前结束 ReadableStream 消费可能导致部分环境下
        // `fetch` 身未完成、外层 `send_chat_stream` 永久 await，状态栏卡「模型生成中」。
    }
    if !raw.is_empty() {
        buffer.push_str(&String::from_utf8_lossy(&raw));
        raw.clear();
    }
    let _tail_meaningful =
        flush_sse_tail(&mut buffer, last_event_id, &mut saw_stream_ended, cbs, loc)?;
    if saw_stream_ended {
        stream_finished_normally = true;
    }
    Ok((stream_finished_normally, saw_stream_ended))
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
    let w = window().ok_or_else(|| "no window".to_string())?;
    let mut last_event_id: u64 = stream_resume_after_seq.unwrap_or(0);
    let mut attempt: u32 = 0;
    loop {
        if signal.aborted() {
            return Ok(());
        }
        let body = build_chat_stream_post_body(
            &message,
            &image_urls,
            &conversation_id,
            &agent_role,
            &approval_session_id,
            stream_resume_job_id,
            last_event_id,
            &clarify_questionnaire_answers,
        )?;
        let body_json = serde_json::to_string(&body).map_err(|e| e.to_string())?;
        let req = build_chat_stream_fetch_request(&body_json, signal, last_event_id)?;
        let resp_val = match JsFuture::from(w.fetch_with_request(&req)).await {
            Ok(v) => v,
            Err(e) => {
                if stream_resume_job_id.is_none() || attempt >= 6 {
                    return Err(format!("fetch: {:?}", e));
                }
                attempt = attempt.saturating_add(1);
                sleep_chat_stream_retry_backoff(attempt).await;
                continue;
            }
        };
        let resp: Response = resp_val.dyn_into().map_err(|_| "not Response")?;
        apply_chat_stream_response_headers(&resp, &cbs, &mut stream_resume_job_id);
        if resp.status() == 410 {
            return Err(crate::i18n::api_err_stream_gone(loc).to_string());
        }
        if !resp.ok() {
            let msg = chat_stream_read_error_body(&resp, loc).await?;
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg)
                && let Some(m) = v.get("message").and_then(|x| x.as_str())
                && !m.trim().is_empty()
            {
                return Err(m.to_string());
            }
            return Err(msg);
        }
        let Some(rb) = resp.body() else {
            return Err(crate::i18n::api_err_no_response_body(loc).to_string());
        };
        let (stream_finished_normally, saw_stream_ended) = consume_chat_stream_response_body(
            rb,
            signal,
            &mut last_event_id,
            &cbs,
            loc,
            stream_resume_job_id,
        )
        .await?;
        if stream_finished_normally {
            if !saw_stream_ended {
                // 某些后端/网络尾部场景可能未显式下发 `stream_ended`，前端按正常完结补齐。
                (cbs.on_stream_ended)(StreamEndReason::Completed.to_string());
            }
            (cbs.on_done)();
            return Ok(());
        }
        if stream_resume_job_id.is_none() {
            return Err(crate::i18n::api_err_no_response_body(loc).to_string());
        }
        attempt = attempt.saturating_add(1);
        if attempt >= 6 {
            return Err(crate::i18n::api_err_request_failed(loc).to_string());
        }
        sleep_chat_stream_retry_backoff(attempt).await;
    }
}

fn process_sse_buffer(
    buffer: &mut String,
    last_event_id: &mut u64,
    saw_stream_ended: &mut bool,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
) -> Result<usize, String> {
    let mut meaningful = 0usize;
    while let Some(pos) = buffer.find("\n\n") {
        let block = buffer[..pos].to_string();
        *buffer = buffer[pos + 2..].to_string();
        if handle_sse_block(&block, last_event_id, saw_stream_ended, cbs, loc)? {
            meaningful = meaningful.saturating_add(1);
        }
    }
    Ok(meaningful)
}

fn flush_sse_tail(
    buffer: &mut String,
    last_event_id: &mut u64,
    saw_stream_ended: &mut bool,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
) -> Result<usize, String> {
    // 勿对尾部缓冲 `trim`：流式正文可能单独落在仅含空格/`data: ` 尾部的帧里，trim 会吞掉词间空格。
    let meaningful = if buffer.is_empty() {
        0usize
    } else if handle_sse_block(buffer.as_str(), last_event_id, saw_stream_ended, cbs, loc)? {
        1
    } else {
        0
    };
    buffer.clear();
    Ok(meaningful)
}

/// `Ok(true)`：本帧带有非空、非 `[DONE]` 的 `data:` 负载，并已走完 `stream_ended` 或控制面/正文分发。
fn handle_sse_block(
    block: &str,
    last_event_id: &mut u64,
    saw_stream_ended: &mut bool,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
) -> Result<bool, String> {
    if let Some(id) = parse_sse_event_id(block) {
        *last_event_id = id;
        (cbs.on_last_sse_event_id)(id);
    }
    let Some(data) = join_sse_data_lines(block) else {
        return Ok(false);
    };
    // 勿对 `data` 全文 `trim`：模型/代理可能把词间空格单独打成一段 SSE，trim 会导致单词粘在一起。
    if data.is_empty() || is_sse_done_sentinel(&data) {
        return Ok(false);
    }
    if let Some(reason) = extract_stream_ended_reason(&data) {
        *saw_stream_ended = true;
        (cbs.on_stream_ended)(reason);
        return Ok(true);
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data)
        && let Some(ended) = v.get("stream_ended")
        && !ended.is_null()
    {
        // `reason` 缺失或非字符串时仍须回落 busy（与 `dispatch_notice_timeline_tail` 吞掉 `stream_ended` 的形态对齐）。
        let reason = ended
            .get("reason")
            .and_then(|x| x.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| StreamEndReason::Completed.to_string());
        *saw_stream_ended = true;
        (cbs.on_stream_ended)(reason);
        return Ok(true);
    }

    let mut stop = false;
    let mut on_err = |msg: String| {
        stop = true;
        (cbs.on_error)(msg);
    };
    let mut on_ws = || (cbs.on_workspace_changed)();
    let mut on_tool_call = |n: String,
                            s: String,
                            p: Option<String>,
                            a: Option<String>,
                            g: Option<String>,
                            tid: Option<String>| {
        (cbs.on_tool_call)(n, s, p, a, g, tid);
    };
    let mut on_tool_status = |b: bool| (cbs.on_tool_status)(b);
    let mut on_parse = |_b: bool| {};
    let mut on_tool_res = |info: ToolResultInfo| (cbs.on_tool_result)(info);
    let mut on_appr = |req: CommandApprovalRequest| (cbs.on_approval)(req);
    let mut on_conv_rev = |rev: u64| (cbs.on_conversation_revision)(rev);
    let mut on_staged_start =
        |info: StagedPlanStepStartInfo| (cbs.on_staged_plan_step_started)(info);
    let mut on_staged_end = |info: StagedPlanStepEndInfo| (cbs.on_staged_plan_step_finished)(info);
    let mut on_clar = |info: crate::sse_dispatch::ClarificationQuestionnaireInfo| {
        (cbs.on_clarification_questionnaire)(info)
    };
    let mut on_phase = || (cbs.on_assistant_answer_phase)();
    let mut on_thinking_trace = |info: ThinkingTraceInfo| (cbs.on_thinking_trace)(info);
    let mut on_timeline_log = |info: TimelineLogInfo| (cbs.on_timeline_log)(info);

    let mut cbs2 = SseCallbacks {
        user_locale: loc,
        on_error: &mut on_err,
        on_workspace_changed: Some(&mut on_ws),
        on_tool_call: Some(&mut on_tool_call),
        on_tool_status_change: Some(&mut on_tool_status),
        on_parsing_tool_calls_change: Some(&mut on_parse),
        on_assistant_answer_phase: Some(&mut on_phase),
        on_tool_result: Some(&mut on_tool_res),
        on_command_approval_request: Some(&mut on_appr),
        on_conversation_saved_revision: Some(&mut on_conv_rev),
        on_staged_plan_step_started: Some(&mut on_staged_start),
        on_staged_plan_step_finished: Some(&mut on_staged_end),
        on_clarification_questionnaire: Some(&mut on_clar),
        on_thinking_trace: Some(&mut on_thinking_trace),
        on_timeline_log: Some(&mut on_timeline_log),
    };
    match try_dispatch_sse_control_payload(&data, &mut cbs2) {
        crate::sse_dispatch::SseDispatch::Stop => Ok(true),
        crate::sse_dispatch::SseDispatch::Handled => {
            if stop {
                Err(crate::i18n::api_err_stream_stopped(loc).to_string())
            } else {
                Ok(true)
            }
        }
        crate::sse_dispatch::SseDispatch::Plain => {
            if stop {
                return Err(crate::i18n::api_err_stream_stopped(loc).to_string());
            }
            (cbs.on_delta)(data);
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatStreamCallbacks, handle_sse_block};
    use crate::i18n::Locale;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn callbacks_with_end_capture(ended: Rc<RefCell<Option<String>>>) -> ChatStreamCallbacks {
        ChatStreamCallbacks {
            on_delta: Rc::new(|_s| {}),
            on_done: Rc::new(|| {}),
            on_error: Rc::new(|_e| {}),
            on_workspace_changed: Rc::new(|| {}),
            on_tool_status: Rc::new(|_b| {}),
            on_tool_result: Rc::new(|_info| {}),
            on_approval: Rc::new(|_req| {}),
            on_conversation_id: Rc::new(|_id| {}),
            on_conversation_revision: Rc::new(|_rev| {}),
            on_stream_ended: Rc::new(move |reason| {
                *ended.borrow_mut() = Some(reason);
            }),
            on_stream_job_id: Rc::new(|_jid| {}),
            on_last_sse_event_id: Rc::new(|_seq| {}),
            on_assistant_answer_phase: Rc::new(|| {}),
            on_staged_plan_step_started: Rc::new(|_info| {}),
            on_staged_plan_step_finished: Rc::new(|_info| {}),
            on_clarification_questionnaire: Rc::new(|_info| {}),
            on_thinking_trace: Rc::new(|_info| {}),
            on_timeline_log: Rc::new(|_info| {}),
            on_tool_call: Rc::new(|_n, _s, _p, _a, _g, _tid| {}),
        }
    }

    #[test]
    fn handle_block_marks_stream_ended_when_reason_present() {
        let ended = Rc::new(RefCell::new(None::<String>));
        let cbs = callbacks_with_end_capture(Rc::clone(&ended));
        let mut last_event_id = 0u64;
        let mut saw_stream_ended = false;
        let block = "id: 12\ndata: {\"stream_ended\":{\"job_id\":1,\"reason\":\"completed\"}}\n\n";
        let res = handle_sse_block(
            block.trim(),
            &mut last_event_id,
            &mut saw_stream_ended,
            &cbs,
            Locale::ZhHans,
        );
        assert!(res.is_ok());
        assert!(saw_stream_ended);
        assert_eq!(last_event_id, 12);
        assert_eq!(ended.borrow().as_deref(), Some("completed"));
    }

    #[test]
    fn handle_block_marks_stream_ended_when_reason_missing_uses_completed() {
        let ended = Rc::new(RefCell::new(None::<String>));
        let cbs = callbacks_with_end_capture(Rc::clone(&ended));
        let mut last_event_id = 0u64;
        let mut saw_stream_ended = false;
        let block = "data: {\"stream_ended\":{\"job_id\":3}}\n\n";
        let res = handle_sse_block(
            block.trim(),
            &mut last_event_id,
            &mut saw_stream_ended,
            &cbs,
            Locale::ZhHans,
        );
        assert!(res.is_ok());
        assert!(saw_stream_ended);
        assert_eq!(ended.borrow().as_deref(), Some("completed"));
    }

    /// `data: ` 后仅空格的增量不得被 `trim_start` 吞掉，否则英文词会粘在一起。
    #[test]
    fn handle_block_preserves_whitespace_only_delta() {
        let got = Rc::new(RefCell::new(String::new()));
        let got2 = Rc::clone(&got);
        let cbs = ChatStreamCallbacks {
            on_delta: Rc::new(move |s| got2.borrow_mut().push_str(&s)),
            on_done: Rc::new(|| {}),
            on_error: Rc::new(|_e| {}),
            on_workspace_changed: Rc::new(|| {}),
            on_tool_status: Rc::new(|_b| {}),
            on_tool_result: Rc::new(|_info| {}),
            on_approval: Rc::new(|_req| {}),
            on_conversation_id: Rc::new(|_id| {}),
            on_conversation_revision: Rc::new(|_rev| {}),
            on_stream_ended: Rc::new(|_reason| {}),
            on_stream_job_id: Rc::new(|_jid| {}),
            on_last_sse_event_id: Rc::new(|_seq| {}),
            on_assistant_answer_phase: Rc::new(|| {}),
            on_staged_plan_step_started: Rc::new(|_info| {}),
            on_staged_plan_step_finished: Rc::new(|_info| {}),
            on_clarification_questionnaire: Rc::new(|_info| {}),
            on_thinking_trace: Rc::new(|_info| {}),
            on_timeline_log: Rc::new(|_info| {}),
            on_tool_call: Rc::new(|_n, _s, _p, _a, _g, _tid| {}),
        };
        let mut last_event_id = 0u64;
        let mut saw_stream_ended = false;
        let block = "data:  \n\n";
        let res = handle_sse_block(
            block,
            &mut last_event_id,
            &mut saw_stream_ended,
            &cbs,
            Locale::ZhHans,
        );
        assert!(res.is_ok());
        assert_eq!(got.borrow().as_str(), " ");
    }
}
