//! 浏览器 `fetch` + `/chat/stream` SSE 解析（单前端实现）。

#![allow(clippy::collapsible_if)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestInit, RequestMode, Response, Window};

use crabmate_sse_protocol::SSE_PROTOCOL_VERSION;

use crate::i18n::Locale;
use crate::sse_dispatch::{
    CommandApprovalRequest, SseCallbacks, ToolResultInfo, try_dispatch_sse_control_payload,
};

const WEB_API_BEARER_TOKEN_KEY: &str = "crabmate-api-bearer-token";

/// Web 设置中保存的 LLM 网关基址（`client_llm.api_base`）。
pub const CLIENT_LLM_API_BASE_STORAGE_KEY: &str = "crabmate-client-llm-api-base";
/// Web 设置中保存的模型名（`client_llm.model`）。
pub const CLIENT_LLM_MODEL_STORAGE_KEY: &str = "crabmate-client-llm-model";
/// Web 设置中保存的云端 API 密钥（`client_llm.api_key`）；**仅存本机**。
pub const CLIENT_LLM_API_KEY_STORAGE_KEY: &str = "crabmate-client-llm-api-key";

fn local_storage() -> Option<web_sys::Storage> {
    window().and_then(|w| w.local_storage().ok().flatten())
}

fn storage_trimmed_item(key: &str) -> Option<String> {
    let st = local_storage()?;
    let s = st.get_item(key).ok().flatten()?;
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// 是否已在 localStorage 保存过 `client_llm.api_key`（不返回密钥内容）。
pub fn client_llm_storage_has_api_key() -> bool {
    storage_trimmed_item(CLIENT_LLM_API_KEY_STORAGE_KEY).is_some()
}

/// 供设置弹窗加载：`api_base` / `model` 的已存值（无则空串）。
pub fn load_client_llm_text_fields_from_storage() -> (String, String) {
    (
        storage_trimmed_item(CLIENT_LLM_API_BASE_STORAGE_KEY).unwrap_or_default(),
        storage_trimmed_item(CLIENT_LLM_MODEL_STORAGE_KEY).unwrap_or_default(),
    )
}

/// 将模型相关设置写入 localStorage。`api_key` 为 `None` 时不改已存密钥；为 `Some("")` 可配合调用方在「清除」时 `remove_item`。
pub fn persist_client_llm_to_storage(
    api_base: &str,
    model: &str,
    api_key_update: Option<&str>,
    loc: Locale,
) -> Result<(), String> {
    let st =
        local_storage().ok_or_else(|| crate::i18n::api_err_no_local_storage(loc).to_string())?;
    let b = api_base.trim();
    let m = model.trim();
    if b.is_empty() {
        let _ = st.remove_item(CLIENT_LLM_API_BASE_STORAGE_KEY);
    } else {
        st.set_item(CLIENT_LLM_API_BASE_STORAGE_KEY, b)
            .map_err(|_| crate::i18n::api_err_write_api_base(loc).to_string())?;
    }
    if m.is_empty() {
        let _ = st.remove_item(CLIENT_LLM_MODEL_STORAGE_KEY);
    } else {
        st.set_item(CLIENT_LLM_MODEL_STORAGE_KEY, m)
            .map_err(|_| crate::i18n::api_err_write_model(loc).to_string())?;
    }
    if let Some(k) = api_key_update {
        let t = k.trim();
        if t.is_empty() {
            let _ = st.remove_item(CLIENT_LLM_API_KEY_STORAGE_KEY);
        } else {
            st.set_item(CLIENT_LLM_API_KEY_STORAGE_KEY, t)
                .map_err(|_| crate::i18n::api_err_write_api_key(loc).to_string())?;
        }
    }
    Ok(())
}

pub fn clear_client_llm_api_key_storage(loc: Locale) -> Result<(), String> {
    let st =
        local_storage().ok_or_else(|| crate::i18n::api_err_no_local_storage(loc).to_string())?;
    let _ = st.remove_item(CLIENT_LLM_API_KEY_STORAGE_KEY);
    Ok(())
}

/// 合并进 `/chat/stream` 请求体的 `client_llm` 对象（省略未配置的字段）。
pub fn client_llm_json_for_chat_body() -> Option<Value> {
    let mut m = serde_json::Map::new();
    if let Some(v) = storage_trimmed_item(CLIENT_LLM_API_BASE_STORAGE_KEY) {
        m.insert("api_base".into(), Value::String(v));
    }
    if let Some(v) = storage_trimmed_item(CLIENT_LLM_MODEL_STORAGE_KEY) {
        m.insert("model".into(), Value::String(v));
    }
    if let Some(v) = storage_trimmed_item(CLIENT_LLM_API_KEY_STORAGE_KEY) {
        m.insert("api_key".into(), Value::String(v));
    }
    if m.is_empty() {
        None
    } else {
        Some(Value::Object(m))
    }
}

fn window() -> Option<Window> {
    web_sys::window()
}

fn auth_headers() -> Headers {
    let h = Headers::new().expect("Headers::new");
    if let Some(st) = local_storage() {
        if let Ok(Some(t)) = st.get_item(WEB_API_BEARER_TOKEN_KEY) {
            let t = t.trim();
            if !t.is_empty() {
                let _ = h.set("Authorization", &format!("Bearer {t}"));
                // 与后端 `require_web_api_bearer_auth` 一致：亦接受 X-API-Key（网关/脚本常用）
                let _ = h.set("X-API-Key", t);
            }
        }
    }
    h
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceEntry {
    pub name: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceData {
    pub path: String,
    pub entries: Vec<WorkspaceEntry>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TaskItem {
    pub id: String,
    pub title: String,
    pub done: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TasksData {
    #[serde(default)]
    pub items: Vec<TaskItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatusData {
    pub model: String,
    pub api_base: String,
    #[serde(default)]
    pub agent_role_ids: Vec<String>,
    #[serde(default)]
    pub default_agent_role_id: Option<String>,
    /// 与后端 `message_pipeline` 按字符删旧一致；`0` 表示未启用预算（进度条仅展示字符数）。
    #[serde(default)]
    pub context_char_budget: usize,
}

pub async fn fetch_workspace(path: Option<&str>) -> Result<WorkspaceData, String> {
    let url = match path {
        Some(p) if !p.trim().is_empty() => format!("/workspace?path={}", urlencoding::encode(p)),
        _ => "/workspace".to_string(),
    };
    fetch_json("GET", &url, None).await
}

#[derive(Debug, Deserialize)]
pub struct WorkspacePickResponse {
    pub path: Option<String>,
}

/// `GET /workspace/pick`：在**运行 crabmate serve 的进程所在机器**上弹出原生「选择文件夹」对话框（`rfd`）。
/// 无图形、无头或用户取消时 `path` 为 `None`。
pub async fn fetch_workspace_pick() -> Result<Option<String>, String> {
    let r: WorkspacePickResponse = fetch_json("GET", "/workspace/pick", None).await?;
    Ok(r.path
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty()))
}

/// 与 **`session_workspace_changelist`** 注入模型正文同源（Markdown）。
#[derive(Debug, Deserialize)]
pub struct WorkspaceChangelogResponse {
    pub revision: u64,
    #[serde(default)]
    pub markdown: String,
    #[serde(default)]
    pub error: Option<String>,
}

/// `GET /workspace/changelog`：可选 `conversation_id` 与 Web 会话作用域对齐。
pub async fn fetch_workspace_changelog(
    conversation_id: Option<&str>,
) -> Result<WorkspaceChangelogResponse, String> {
    let url = match conversation_id {
        Some(id) if !id.trim().is_empty() => format!(
            "/workspace/changelog?conversation_id={}",
            urlencoding::encode(id.trim())
        ),
        _ => "/workspace/changelog".to_string(),
    };
    fetch_json("GET", &url, None).await
}

#[derive(Serialize)]
struct WorkspaceSetBody {
    /// `None`：省略字段，服务端按「恢复默认工作目录」处理。
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

/// `POST /workspace`：设置当前 Web 会话工作区根。`path: None` 表示恢复服务端默认（`run_command_working_dir`）。
/// 成功返回规范化后的路径字符串（可能为空，表示默认）。
pub async fn post_workspace_set(path: Option<String>, loc: Locale) -> Result<String, String> {
    let body = serde_json::to_string(&WorkspaceSetBody { path }).map_err(|e| e.to_string())?;
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    let _ = h.set("Content-Type", "application/json");
    init.set_headers(&h);
    init.set_body(&JsValue::from_str(&body));
    let req = Request::new_with_str_and_init("/workspace", &init)
        .map_err(|e| format!("request: {:?}", e))?;
    let w = window().ok_or_else(|| "no window".to_string())?;
    let resp_val = JsFuture::from(w.fetch_with_request(&req))
        .await
        .map_err(|e| format!("fetch: {:?}", e))?;
    let resp: Response = resp_val.dyn_into().map_err(|_| "not Response")?;
    let text = JsFuture::from(resp.text().map_err(|e| format!("text: {:?}", e))?)
        .await
        .map_err(|e| format!("read body: {:?}", e))?;
    let s = text
        .as_string()
        .ok_or_else(|| "body not string".to_string())?;
    let v: Value = serde_json::from_str(&s).map_err(|_| format!("HTTP {} {}", resp.status(), s))?;
    if resp.ok() {
        if v.get("ok").and_then(|x| x.as_bool()) != Some(true) {
            return Err(v
                .get("error")
                .and_then(|x| x.as_str())
                .unwrap_or(crate::i18n::api_err_workspace_set_failed(loc))
                .to_string());
        }
        return Ok(v
            .get("path")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string());
    }
    Err(v
        .get("error")
        .and_then(|x| x.as_str())
        .map(std::string::ToString::to_string)
        .unwrap_or_else(|| format!("HTTP {}", resp.status())))
}

pub async fn fetch_tasks() -> Result<TasksData, String> {
    fetch_json("GET", "/tasks", None).await
}

pub async fn fetch_status() -> Result<StatusData, String> {
    fetch_json("GET", "/status", None).await
}

pub async fn save_tasks(data: &TasksData) -> Result<TasksData, String> {
    let body = serde_json::to_string(data).map_err(|e| e.to_string())?;
    fetch_json_with_body("POST", "/tasks", &body).await
}

async fn fetch_json<T: for<'de> Deserialize<'de>>(
    method: &str,
    url: &str,
    _body: Option<&str>,
) -> Result<T, String> {
    let init = RequestInit::new();
    init.set_method(method);
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    init.set_headers(&h);
    let req =
        Request::new_with_str_and_init(url, &init).map_err(|e| format!("request: {:?}", e))?;
    do_fetch_json(req).await
}

async fn fetch_json_with_body<T: for<'de> Deserialize<'de>>(
    method: &str,
    url: &str,
    body: &str,
) -> Result<T, String> {
    let init = RequestInit::new();
    init.set_method(method);
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    let _ = h.set("Content-Type", "application/json");
    init.set_headers(&h);
    init.set_body(&wasm_bindgen::JsValue::from_str(body));
    let req =
        Request::new_with_str_and_init(url, &init).map_err(|e| format!("request: {:?}", e))?;
    do_fetch_json(req).await
}

async fn do_fetch_json<T: for<'de> Deserialize<'de>>(req: Request) -> Result<T, String> {
    let w = window().ok_or_else(|| "no window".to_string())?;
    let p = w.fetch_with_request(&req);
    let resp_val = JsFuture::from(p)
        .await
        .map_err(|e| format!("fetch: {:?}", e))?;
    let resp: Response = resp_val.dyn_into().map_err(|_| "not Response")?;
    if !resp.ok() {
        let text = JsFuture::from(resp.text().map_err(|e| format!("text: {:?}", e))?)
            .await
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        return Err(format!("HTTP {} {}", resp.status(), text));
    }
    let text = JsFuture::from(resp.text().map_err(|e| format!("text: {:?}", e))?)
        .await
        .map_err(|e| format!("read body: {:?}", e))?;
    let s = text
        .as_string()
        .ok_or_else(|| "body not string".to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

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
        }
    }
}

/// `/chat/stream`：支持 **`Last-Event-ID`** 与 JSON **`stream_resume`** 断线重连（网络抖动时自动重试若干次）。
pub async fn send_chat_stream(
    message: String,
    conversation_id: Option<String>,
    agent_role: Option<String>,
    approval_session_id: Option<String>,
    mut stream_resume_job_id: Option<u64>,
    stream_resume_after_seq: Option<u64>,
    signal: &web_sys::AbortSignal,
    cbs: ChatStreamCallbacks,
    loc: Locale,
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
        if let Some(jid) = stream_resume_job_id {
            body["stream_resume"] = serde_json::json!({
                "job_id": jid,
                "after_seq": last_event_id,
            });
        }
        if let Some(cl) = client_llm_json_for_chat_body() {
            body["client_llm"] = cl;
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
            return Err("流式任务已结束或不在服务端内存中，无法重连".to_string());
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
            .map_err(|_| "stream reader".to_string())?;

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
                        return Err(format!("read await: {:?}", e));
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
            process_sse_buffer(&mut buffer, &mut last_event_id, &cbs)?;
        }
        if !raw.is_empty() {
            buffer.push_str(&String::from_utf8_lossy(&raw));
            raw.clear();
        }
        flush_sse_tail(&mut buffer, &mut last_event_id, &cbs)?;
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
) -> Result<(), String> {
    while let Some(pos) = buffer.find("\n\n") {
        let block = buffer[..pos].to_string();
        *buffer = buffer[pos + 2..].to_string();
        handle_sse_block(&block, last_event_id, cbs)?;
    }
    Ok(())
}

fn flush_sse_tail(
    buffer: &mut String,
    last_event_id: &mut u64,
    cbs: &ChatStreamCallbacks,
) -> Result<(), String> {
    let t = buffer.trim();
    if !t.is_empty() {
        handle_sse_block(t, last_event_id, cbs)?;
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
    let mut on_tool_call = |_n: String, _s: String| {};
    let mut on_tool_status = |b: bool| (cbs.on_tool_status)(b);
    let mut on_parse = |_b: bool| {};
    let mut on_tool_res = |info: ToolResultInfo| (cbs.on_tool_result)(info);
    let mut on_appr = |req: CommandApprovalRequest| (cbs.on_approval)(req);
    let mut on_conv_rev = |rev: u64| (cbs.on_conversation_revision)(rev);

    let mut cbs2 = SseCallbacks {
        on_error: &mut on_err,
        on_workspace_changed: Some(&mut on_ws),
        on_tool_call: Some(&mut on_tool_call),
        on_tool_status_change: Some(&mut on_tool_status),
        on_parsing_tool_calls_change: Some(&mut on_parse),
        on_tool_result: Some(&mut on_tool_res),
        on_command_approval_request: Some(&mut on_appr),
        on_conversation_saved_revision: Some(&mut on_conv_rev),
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
                Err("stream stopped".to_string())
            } else {
                Ok(())
            }
        }
        crate::sse_dispatch::SseDispatch::Plain => {
            if stop {
                return Err("stream stopped".to_string());
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

#[derive(Serialize)]
struct ApprovalBody<'a> {
    approval_session_id: &'a str,
    decision: &'a str,
}

#[derive(Debug, Serialize)]
struct ChatBranchBody {
    conversation_id: String,
    before_user_ordinal: u64,
    expected_revision: u64,
}

#[derive(Debug, Deserialize)]
pub struct ChatBranchResponse {
    pub ok: bool,
    #[serde(default)]
    pub revision: u64,
}

/// `POST /chat/branch`：服务端按 `before_user_ordinal` 截断持久化会话（须 `conversation_store_sqlite_path` 等已启用）。
pub async fn post_chat_branch(
    conversation_id: &str,
    before_user_ordinal: u64,
    expected_revision: u64,
    loc: Locale,
) -> Result<u64, String> {
    let body = serde_json::to_string(&ChatBranchBody {
        conversation_id: conversation_id.to_string(),
        before_user_ordinal,
        expected_revision,
    })
    .map_err(|e| e.to_string())?;
    let r: ChatBranchResponse = fetch_json_with_body("POST", "/chat/branch", &body).await?;
    if !r.ok {
        return Err(crate::i18n::api_err_branch_failed(loc).to_string());
    }
    Ok(r.revision)
}

pub async fn submit_chat_approval(
    session_id: &str,
    decision: &str,
    loc: Locale,
) -> Result<(), String> {
    let body = serde_json::to_string(&ApprovalBody {
        approval_session_id: session_id,
        decision,
    })
    .map_err(|e| e.to_string())?;
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    let _ = h.set("Content-Type", "application/json");
    init.set_headers(&h);
    init.set_body(&wasm_bindgen::JsValue::from_str(&body));
    let req = Request::new_with_str_and_init("/chat/approval", &init)
        .map_err(|e| format!("req: {:?}", e))?;
    let w = window().ok_or_else(|| "no window".to_string())?;
    let resp_val = JsFuture::from(w.fetch_with_request(&req))
        .await
        .map_err(|e| format!("fetch: {:?}", e))?;
    let resp: Response = resp_val.dyn_into().map_err(|_| "not Response")?;
    if !resp.ok() {
        return Err(crate::i18n::api_err_approval_failed(loc, resp.status()));
    }
    Ok(())
}
