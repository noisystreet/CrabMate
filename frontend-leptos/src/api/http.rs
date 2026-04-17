//! JSON over `fetch`：工作区、任务、上传、会话分支与审批等（不含 `/chat/stream` SSE）。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

use crate::i18n::Locale;

use super::browser::{auth_headers, window};

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
    #[allow(dead_code)]
    pub context_char_budget: usize,
    #[serde(default)]
    pub executor_model: String,
    #[serde(default)]
    pub executor_api_base: String,
}

fn default_markdown_render_true() -> bool {
    true
}

fn default_apply_assistant_display_filters_true() -> bool {
    true
}

/// `GET /web-ui`：服务端由环境变量导出的 CSR 展示开关（无 TOML 字段）。
#[derive(Debug, Clone, Deserialize)]
pub struct WebUiConfig {
    /// 为 `false` 时关闭聊天气泡与变更集模态的 Markdown 渲染（纯文本 HTML 转义）。
    #[serde(default = "default_markdown_render_true")]
    pub markdown_render: bool,
    /// 为 `false` 时助手消息按存储原文展示（不剥 `agent_reply_plan`、不拆内联思维链标记）；与导出、搜索一致。
    #[serde(default = "default_apply_assistant_display_filters_true")]
    pub apply_assistant_display_filters: bool,
}

pub async fn fetch_workspace(path: Option<&str>, loc: Locale) -> Result<WorkspaceData, String> {
    let url = match path {
        Some(p) if !p.trim().is_empty() => format!("/workspace?path={}", urlencoding::encode(p)),
        _ => "/workspace".to_string(),
    };
    fetch_json("GET", &url, None, loc).await
}

#[derive(Debug, Deserialize)]
pub struct WorkspacePickResponse {
    pub path: Option<String>,
}

/// `GET /workspace/pick`：在**运行 crabmate serve 的进程所在机器**上弹出原生「选择文件夹」对话框（`rfd`）。
/// 无图形、无头或用户取消时 `path` 为 `None`。
pub async fn fetch_workspace_pick(loc: Locale) -> Result<Option<String>, String> {
    let r: WorkspacePickResponse = fetch_json("GET", "/workspace/pick", None, loc).await?;
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
    loc: Locale,
) -> Result<WorkspaceChangelogResponse, String> {
    let url = match conversation_id {
        Some(id) if !id.trim().is_empty() => format!(
            "/workspace/changelog?conversation_id={}",
            urlencoding::encode(id.trim())
        ),
        _ => "/workspace/changelog".to_string(),
    };
    fetch_json("GET", &url, None, loc).await
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
    let w = window().ok_or_else(|| crate::i18n::api_err_no_window(loc).to_string())?;
    let resp_val = JsFuture::from(w.fetch_with_request(&req))
        .await
        .map_err(|e| format!("fetch: {:?}", e))?;
    let resp: Response = resp_val
        .dyn_into()
        .map_err(|_| crate::i18n::api_err_response_type(loc))?;
    let text = JsFuture::from(resp.text().map_err(|e| format!("text: {:?}", e))?)
        .await
        .map_err(|e| format!("read body: {:?}", e))?;
    let s = text
        .as_string()
        .ok_or_else(|| crate::i18n::api_err_body_type(loc).to_string())?;
    let v: Value =
        serde_json::from_str(&s).map_err(|_| crate::i18n::api_err_request_failed(loc))?;
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

pub async fn fetch_tasks(loc: Locale) -> Result<TasksData, String> {
    fetch_json("GET", "/tasks", None, loc).await
}

pub async fn fetch_status(loc: Locale) -> Result<StatusData, String> {
    fetch_json("GET", "/status", None, loc).await
}

pub async fn fetch_web_ui_config(loc: Locale) -> Result<WebUiConfig, String> {
    fetch_json("GET", "/web-ui", None, loc).await
}

pub async fn save_tasks(data: &TasksData, loc: Locale) -> Result<TasksData, String> {
    let body = serde_json::to_string(data).map_err(|e| e.to_string())?;
    fetch_json_with_body("POST", "/tasks", &body, loc).await
}

async fn fetch_json<T: for<'de> Deserialize<'de>>(
    method: &str,
    url: &str,
    _body: Option<&str>,
    loc: Locale,
) -> Result<T, String> {
    let init = RequestInit::new();
    init.set_method(method);
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    init.set_headers(&h);
    let req =
        Request::new_with_str_and_init(url, &init).map_err(|e| format!("request: {:?}", e))?;
    do_fetch_json(req, loc).await
}

async fn fetch_json_with_body<T: for<'de> Deserialize<'de>>(
    method: &str,
    url: &str,
    body: &str,
    loc: Locale,
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
    do_fetch_json(req, loc).await
}

async fn do_fetch_json<T: for<'de> Deserialize<'de>>(
    req: Request,
    loc: Locale,
) -> Result<T, String> {
    let w = window().ok_or_else(|| crate::i18n::api_err_no_window(loc).to_string())?;
    let p = w.fetch_with_request(&req);
    let resp_val = JsFuture::from(p)
        .await
        .map_err(|e| format!("fetch: {:?}", e))?;
    let resp: Response = resp_val
        .dyn_into()
        .map_err(|_| crate::i18n::api_err_response_type(loc))?;
    if !resp.ok() {
        return Err(crate::i18n::api_err_request_failed(loc).to_string());
    }
    let text = JsFuture::from(resp.text().map_err(|e| format!("text: {:?}", e))?)
        .await
        .map_err(|e| format!("read body: {:?}", e))?;
    let s = text
        .as_string()
        .ok_or_else(|| crate::i18n::api_err_body_type(loc).to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

#[derive(Debug, serde::Deserialize)]
pub struct UploadedFileInfo {
    pub url: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub filename: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub mime: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub size: u64,
}

#[derive(Debug, serde::Deserialize)]
pub struct UploadResponseBody {
    pub files: Vec<UploadedFileInfo>,
}

/// `POST /upload`：`multipart/form-data`，字段名任意；返回的 `url` 为 `/uploads/...`。
pub async fn upload_files_multipart(
    form: &web_sys::FormData,
    loc: Locale,
) -> Result<Vec<String>, String> {
    let w = window().ok_or_else(|| crate::i18n::api_err_no_window(loc).to_string())?;
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_mode(RequestMode::Cors);
    init.set_body(form);
    let h = auth_headers();
    init.set_headers(&h);
    let req = Request::new_with_str_and_init("/upload", &init)
        .map_err(|e| format!("request: {:?}", e))?;
    let resp_val = JsFuture::from(w.fetch_with_request(&req))
        .await
        .map_err(|e| format!("fetch: {:?}", e))?;
    let resp: Response = resp_val
        .dyn_into()
        .map_err(|_| crate::i18n::api_err_response_type(loc))?;
    if !resp.ok() {
        let text = JsFuture::from(resp.text().map_err(|e| format!("text: {:?}", e))?)
            .await
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        return Err(text);
    }
    let text = JsFuture::from(resp.text().map_err(|e| format!("text: {:?}", e))?)
        .await
        .map_err(|e| format!("read body: {:?}", e))?;
    let s = text
        .as_string()
        .ok_or_else(|| crate::i18n::api_err_body_type(loc).to_string())?;
    let body: UploadResponseBody = serde_json::from_str(&s).map_err(|e| e.to_string())?;
    Ok(body.files.into_iter().map(|f| f.url).collect())
}

#[derive(Debug, Serialize)]
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
/// `GET /conversation/messages`：拉取服务端已持久化会话（与 `conversation_id` + `revision` 对齐）。
pub async fn fetch_conversation_messages(
    conversation_id: &str,
    loc: Locale,
) -> Result<crate::conversation_hydrate::ConversationMessagesResponse, String> {
    let enc = urlencoding::encode(conversation_id);
    let url = format!("/conversation/messages?conversation_id={enc}");
    fetch_json("GET", &url, None, loc).await
}

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
    let r: ChatBranchResponse = fetch_json_with_body("POST", "/chat/branch", &body, loc).await?;
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
    let w = window().ok_or_else(|| crate::i18n::api_err_no_window(loc).to_string())?;
    let resp_val = JsFuture::from(w.fetch_with_request(&req))
        .await
        .map_err(|e| format!("fetch: {:?}", e))?;
    let resp: Response = resp_val
        .dyn_into()
        .map_err(|_| crate::i18n::api_err_response_type(loc))?;
    if !resp.ok() {
        return Err(crate::i18n::api_err_approval_failed(loc, resp.status()));
    }
    Ok(())
}
