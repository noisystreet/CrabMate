//! `/chat/stream`：`fetch` + SSE 帧解析与 `sse_dispatch` 桥接。

use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

use crabmate_sse_protocol::SSE_PROTOCOL_VERSION;

use crate::i18n::Locale;
use crate::sse_dispatch::{
    CommandApprovalRequest, SseCallbacks, StagedPlanStepEndInfo, StagedPlanStepStartInfo,
    ThinkingTraceInfo, TimelineLogInfo, ToolResultInfo, try_dispatch_sse_control_payload,
};

use super::browser::{auth_headers, window};
use super::client_llm_storage::{client_llm_json_for_chat_body, executor_llm_json_for_chat_body};

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
    /// 收到 `stream_ended` 控制面时调用（`reason` 如 `completed` / `cancelled`）。
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
        }
    }
}

/// `/chat/stream`：支持 **`Last-Event-ID`** 与 JSON **`stream_resume`** 断线重连（网络抖动时自动重试若干次）。
#[allow(clippy::too_many_arguments)] // 流式聊天入口：正文、图片、会话、审批、断线续传、回调与语言等正交参数
pub async fn send_chat_stream(
    message: String,
    image_urls: Vec<String>,
    conversation_id: Option<String>,
    agent_role: Option<String>,
    approval_session_id: Option<String>,
    mut stream_resume_job_id: Option<u64>,
    stream_resume_after_seq: Option<u64>,
    signal: &web_sys::AbortSignal,
    cbs: ChatStreamCallbacks,
    loc: Locale,
    // 可选：`POST /chat/stream` 的 `clarify_questionnaire_answers`（`questionnaire_id` + `answers`）。
    clarify_questionnaire_answers: Option<serde_json::Value>,
) -> Result<(), String> {
    let w = window().ok_or_else(|| "no window".to_string())?;
    let mut last_event_id: u64 = stream_resume_after_seq.unwrap_or(0);
    let mut attempt: u32 = 0;
    loop {
        if signal.aborted() {
            return Ok(());
        }
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
        if let Some(ref cq) = clarify_questionnaire_answers {
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
        init.set_body(&wasm_bindgen::JsValue::from_str(
            &serde_json::to_string(&body).map_err(|e| e.to_string())?,
        ));
        let req = Request::new_with_str_and_init("/chat/stream", &init)
            .map_err(|e| format!("req: {:?}", e))?;
        let resp_val = match JsFuture::from(w.fetch_with_request(&req)).await {
            Ok(v) => v,
            Err(e) => {
                if stream_resume_job_id.is_none() || attempt >= 6 {
                    return Err(format!("fetch: {:?}", e));
                }
                attempt = attempt.saturating_add(1);
                let ms = (200u64).saturating_mul(1u64 << attempt.min(5));
                gloo_timers::future::TimeoutFuture::new(ms as u32).await;
                continue;
            }
        };
        let resp: Response = resp_val.dyn_into().map_err(|_| "not Response")?;
        if let Some(cid) = resp.headers().get("x-conversation-id").ok().flatten() {
            let t = cid.trim();
            if !t.is_empty() {
                (cbs.on_conversation_id)(t.to_string());
            }
        }
        if let Some(jh) = resp.headers().get("x-stream-job-id").ok().flatten() {
            if let Ok(jid) = jh.trim().parse::<u64>() {
                stream_resume_job_id = Some(jid);
                (cbs.on_stream_job_id)(jid);
            }
        }
        if resp.status() == 410 {
            return Err(crate::i18n::api_err_stream_gone(loc).to_string());
        }
        if !resp.ok() {
            let msg = JsFuture::from(resp.text().map_err(|e| format!("text: {:?}", e))?)
                .await
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_else(|| crate::i18n::api_err_request_failed(loc).to_string());
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
        let reader: web_sys::ReadableStreamDefaultReader = rb
            .get_reader()
            .dyn_into()
            .map_err(|_| crate::i18n::api_err_stream_reader(loc).to_string())?;

        // 块边界可能截断 UTF-8：只把从开头起「完整码点」前缀解码进 `text`，余字节留在 `raw`。
        // 使用 `Utf8Error::valid_up_to` 一次确定合法前缀，避免对每个字节反复 `from_utf8`（原 while 递减为 O(n²)）。
        // SSE 仍由下方 `process_sse_buffer` 按 `\n\n` 分帧；ReadableStream 块与 UTF-8/行边界无关，只能缓冲后解码。
        fn append_chunk_to_text_buffer(raw: &mut Vec<u8>, chunk: &[u8], text: &mut String) {
            raw.extend_from_slice(chunk);
            loop {
                if raw.is_empty() {
                    break;
                }
                match std::str::from_utf8(raw) {
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
                        // `valid_up_to` 保证 `raw[..n]` 为合法 UTF-8 且落在码点边界上。
                        text.push_str(std::str::from_utf8(&raw[..n]).expect("valid_up_to"));
                        raw.drain(..n);
                    }
                }
            }
        }

        let mut raw: Vec<u8> = Vec::new();
        let mut buffer = String::new();
        let mut stream_finished_normally = false;
        loop {
            if signal.aborted() {
                return Ok(());
            }
            let read_promise = reader.read();
            let chunk: wasm_bindgen::JsValue = match JsFuture::from(read_promise).await {
                Ok(c) => c,
                Err(e) => {
                    if stream_resume_job_id.is_none() {
                        return Err(crate::i18n::api_err_stream_read(&e));
                    }
                    break;
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
            process_sse_buffer(&mut buffer, &mut last_event_id, &cbs, loc)?;
        }
        if !raw.is_empty() {
            buffer.push_str(&String::from_utf8_lossy(&raw));
            raw.clear();
        }
        flush_sse_tail(&mut buffer, &mut last_event_id, &cbs, loc)?;
        if stream_finished_normally {
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
        let ms = (200u64).saturating_mul(1u64 << attempt.min(5));
        gloo_timers::future::TimeoutFuture::new(ms as u32).await;
    }
}

fn process_sse_buffer(
    buffer: &mut String,
    last_event_id: &mut u64,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
) -> Result<(), String> {
    while let Some(pos) = buffer.find("\n\n") {
        let block = buffer[..pos].to_string();
        *buffer = buffer[pos + 2..].to_string();
        handle_sse_block(&block, last_event_id, cbs, loc)?;
    }
    Ok(())
}

fn flush_sse_tail(
    buffer: &mut String,
    last_event_id: &mut u64,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
) -> Result<(), String> {
    let t = buffer.trim();
    if !t.is_empty() {
        handle_sse_block(t, last_event_id, cbs, loc)?;
    }
    buffer.clear();
    Ok(())
}

fn parse_sse_event_id_block(block: &str) -> Option<u64> {
    for line in block.lines() {
        let t = line.trim_start();
        let rest = t.strip_prefix("id:")?;
        let s = rest.trim();
        if let Ok(n) = s.parse::<u64>() {
            return Some(n);
        }
    }
    None
}

fn handle_sse_block(
    block: &str,
    last_event_id: &mut u64,
    cbs: &ChatStreamCallbacks,
    loc: Locale,
) -> Result<(), String> {
    if let Some(id) = parse_sse_event_id_block(block) {
        *last_event_id = id;
        (cbs.on_last_sse_event_id)(id);
    }
    let data_lines: Vec<&str> = block.lines().filter(|l| l.starts_with("data: ")).collect();
    if data_lines.is_empty() {
        return Ok(());
    }
    let data = data_lines
        .iter()
        .map(|l| l[6..].trim_start())
        .collect::<Vec<_>>()
        .join("\n");
    let data = data.trim();
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }

    let mut stop = false;
    let mut on_err = |msg: String| {
        stop = true;
        (cbs.on_error)(msg);
    };
    let mut on_ws = || (cbs.on_workspace_changed)();
    let mut on_tool_call = |_n: String, _s: String, _p: Option<String>, _a: Option<String>| {};
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
    match try_dispatch_sse_control_payload(data, &mut cbs2) {
        crate::sse_dispatch::SseDispatch::Stop => Ok(()),
        crate::sse_dispatch::SseDispatch::Handled => {
            if let Ok(v) = serde_json::from_str::<Value>(data)
                && let Some(obj) = v.as_object()
                && key_present_non_null_sse(obj, "stream_ended")
                && let Some(Value::Object(ended)) = obj.get("stream_ended")
                && let Some(Value::String(reason)) = ended.get("reason")
            {
                (cbs.on_stream_ended)(reason.clone());
            }
            if stop {
                Err(crate::i18n::api_err_stream_stopped(loc).to_string())
            } else {
                Ok(())
            }
        }
        crate::sse_dispatch::SseDispatch::Plain => {
            if stop {
                return Err(crate::i18n::api_err_stream_stopped(loc).to_string());
            }
            (cbs.on_delta)(data.to_string());
            Ok(())
        }
    }
}

fn key_present_non_null_sse(obj: &serde_json::Map<String, Value>, key: &str) -> bool {
    match obj.get(key) {
        None | Some(Value::Null) => false,
        Some(_) => true,
    }
}
