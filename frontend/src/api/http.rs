//! JSON over `fetch`：工作区、任务、上传、会话分支与审批等（不含 `/chat/stream` SSE）。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

use crate::i18n::Locale;

use super::browser::{auth_headers, window};

fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
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
    #[allow(dead_code)]
    pub context_char_budget: usize,
    #[serde(default)]
    pub llm_context_tokens: u32,
    /// 会话同步管道实际采用的近似字符预算（与后端 `AgentConfig::effective_context_char_budget_for_pipeline` 一致）。
    #[serde(default)]
    #[allow(dead_code)]
    pub effective_context_char_budget: usize,
    /// 与后端 `GET /status` 的 `tiktoken_prompt_counting_model` 一致。
    #[serde(default)]
    #[allow(dead_code)]
    pub tiktoken_prompt_counting_model: String,
    /// 新会话仅 `system` 时的 prompt token 粗估（键为角色 id，空串为默认）。
    #[serde(default)]
    pub tiktoken_new_session_baseline_by_agent_role: std::collections::BTreeMap<String, u32>,
    #[serde(default)]
    pub executor_model: String,
    #[serde(default)]
    pub executor_api_base: String,
    #[serde(default)]
    pub planner_executor_mode: String,
    /// 服务端是否配置了非空的 `conversation_store_sqlite_path`。
    #[serde(default)]
    pub conversation_store_sqlite_path_configured: bool,
    /// 当前进程是否使用 SQLite 作为 Web 会话后端。
    #[serde(default)]
    pub conversation_store_sqlite_active: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GithubRepoContextData {
    #[serde(default)]
    pub connected: bool,
    #[serde(default)]
    pub is_git_repo: bool,
    #[serde(default)]
    pub gh_available: bool,
    pub repo: Option<String>,
    pub url: Option<String>,
    pub default_branch: Option<String>,
    pub current_branch: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GithubPrItem {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub url: Option<String>,
    pub head_ref: Option<String>,
    pub base_ref: Option<String>,
    pub is_draft: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GithubPrsData {
    #[serde(default)]
    pub items: Vec<GithubPrItem>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GithubPrCheckItem {
    pub name: String,
    pub state: String,
    pub bucket: Option<String>,
    pub link: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GithubChecksSummary {
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub passing: usize,
    #[serde(default)]
    pub failing: usize,
    #[serde(default)]
    pub pending: usize,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GithubPrCurrentChecksData {
    pub pr_number: Option<u64>,
    pub pr_title: Option<String>,
    pub pr_url: Option<String>,
    #[serde(default)]
    pub checks: Vec<GithubPrCheckItem>,
    #[serde(default)]
    pub summary: GithubChecksSummary,
    #[serde(default)]
    pub error: Option<String>,
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

/// `GET /workspace/file?path=…`：读取工作区内文本文件（与后端 `WorkspaceFileReadResponse` 一致）。
#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceFileReadData {
    pub content: String,
    pub error: Option<String>,
}

pub async fn fetch_workspace_file(
    path: &str,
    encoding: Option<&str>,
    loc: Locale,
) -> Result<WorkspaceFileReadData, String> {
    let mut url = format!("/workspace/file?path={}", urlencoding::encode(path));
    if let Some(enc) = encoding.filter(|e| !e.trim().is_empty()) {
        url.push_str("&encoding=");
        url.push_str(&urlencoding::encode(enc.trim()));
    }
    fetch_json("GET", &url, None, loc).await
}

#[derive(Serialize)]
struct WorkspaceFileWritePayload {
    path: String,
    content: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    create_only: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    update_only: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    create_directory: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    parents: bool,
}

/// `POST /workspace/file` 写入响应。
#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceFileWriteData {
    pub error: Option<String>,
}

/// 保存工作区文件（创建或覆盖；与后端 `WorkspaceFileWriteBody` 默认语义一致）。
pub async fn post_workspace_file_write(
    path: String,
    content: String,
    loc: Locale,
) -> Result<(), String> {
    post_workspace_file_write_opts(path, content, false, false, loc).await
}

/// 带 `create_only` / `update_only` 标志的工作区文件写入。
pub async fn post_workspace_file_write_opts(
    path: String,
    content: String,
    create_only: bool,
    update_only: bool,
    loc: Locale,
) -> Result<(), String> {
    let body = serde_json::to_string(&WorkspaceFileWritePayload {
        path,
        content,
        create_only,
        update_only,
        create_directory: false,
        parents: false,
    })
    .map_err(|e| e.to_string())?;
    let r: WorkspaceFileWriteData =
        fetch_json_with_body("POST", "/workspace/file", &body, loc).await?;
    if let Some(e) = r.error {
        return Err(e);
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
struct WorkspaceDirOpResponse {
    error: Option<String>,
}

#[derive(Serialize)]
struct WorkspaceDirCreatePayload {
    path: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    parents: bool,
}

#[derive(Serialize)]
struct WorkspaceDirDeletePayload {
    path: String,
    delete: bool,
    confirm: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    recursive: bool,
}

fn http_error_status_code(err: &str) -> Option<u16> {
    let open = err.find('(')?;
    let close = err.find(')')?;
    err.get(open + 1..close)?.parse().ok()
}

fn is_workspace_dir_route_unavailable(err: &str) -> bool {
    matches!(http_error_status_code(err), Some(404 | 405))
}

fn is_create_directory_field_unsupported(err: &str) -> bool {
    http_error_status_code(err) == Some(422) && err.contains("create_directory")
}

/// 旧后端无目录 API 时：写入 `{path}/.gitkeep`（`create_dir_all` 会创建父目录）。
async fn post_workspace_dir_via_gitkeep(path: String, loc: Locale) -> Result<(), String> {
    let path = path.trim().trim_end_matches('/');
    if path.is_empty() {
        return Err(match loc {
            Locale::ZhHans => "目录名不能为空".to_string(),
            Locale::En => "Directory name cannot be empty".to_string(),
        });
    }
    let keep = format!("{path}/.gitkeep");
    match post_workspace_file_write_opts(keep, String::new(), true, false, loc).await {
        Ok(()) => Ok(()),
        Err(e) if e.contains("已存在") || e.to_ascii_lowercase().contains("already exists") => {
            Ok(())
        }
        Err(e) => Err(e),
    }
}

async fn post_workspace_dir_via_dir_route(
    path: &str,
    parents: bool,
    loc: Locale,
) -> Result<(), String> {
    let body = serde_json::to_string(&WorkspaceDirCreatePayload {
        path: path.to_string(),
        parents,
    })
    .map_err(|e| e.to_string())?;
    let r: WorkspaceDirOpResponse =
        fetch_json_with_body("POST", "/workspace/dir", &body, loc).await?;
    if let Some(e) = r.error {
        return Err(e);
    }
    Ok(())
}

async fn post_workspace_dir_via_file_route(
    path: String,
    parents: bool,
    loc: Locale,
) -> Result<(), String> {
    let body = serde_json::to_string(&WorkspaceFileWritePayload {
        path,
        content: String::new(),
        create_only: false,
        update_only: false,
        create_directory: true,
        parents,
    })
    .map_err(|e| e.to_string())?;
    let r: WorkspaceFileWriteData =
        fetch_json_with_body("POST", "/workspace/file", &body, loc).await?;
    if let Some(e) = r.error {
        return Err(e);
    }
    Ok(())
}

/// `POST /workspace/dir`：在工作区内创建目录（专用路由 → `create_directory` → `.gitkeep` 兼容）。
pub async fn post_workspace_dir(path: String, parents: bool, loc: Locale) -> Result<(), String> {
    match post_workspace_dir_via_dir_route(path.as_str(), parents, loc).await {
        Ok(()) => return Ok(()),
        Err(e) if is_workspace_dir_route_unavailable(&e) => {}
        Err(e) => return Err(e),
    }
    match post_workspace_dir_via_file_route(path.clone(), parents, loc).await {
        Ok(()) => return Ok(()),
        Err(e) if is_create_directory_field_unsupported(&e) => {}
        Err(e) if is_workspace_dir_route_unavailable(&e) => {}
        Err(e) => return Err(e),
    }
    post_workspace_dir_via_gitkeep(path, loc).await
}

/// `DELETE /workspace/file?path=…`：删除工作区内的文件（不支持目录）。
pub async fn delete_workspace_file(path: &str, loc: Locale) -> Result<(), String> {
    let url = format!("/workspace/file?path={}", urlencoding::encode(path));
    let r: WorkspaceDirOpResponse = fetch_json("DELETE", &url, None, loc).await?;
    if let Some(e) = r.error {
        return Err(e);
    }
    Ok(())
}

/// `DELETE /workspace/dir?path=…&confirm=true&recursive=…`：删除工作区目录。
/// 旧后端无 `DELETE` 时回退为 `POST /workspace/dir`（JSON `delete=true`）。
pub async fn delete_workspace_dir(path: &str, recursive: bool, loc: Locale) -> Result<(), String> {
    match delete_workspace_dir_via_delete(path, recursive, loc).await {
        Ok(()) => Ok(()),
        Err(e) if is_workspace_dir_route_unavailable(&e) => {
            delete_workspace_dir_via_post(path, recursive, loc).await
        }
        Err(e) => Err(e),
    }
}

async fn delete_workspace_dir_via_delete(
    path: &str,
    recursive: bool,
    loc: Locale,
) -> Result<(), String> {
    let mut url = format!(
        "/workspace/dir?path={}&confirm=true",
        urlencoding::encode(path)
    );
    if recursive {
        url.push_str("&recursive=true");
    }
    let r: WorkspaceDirOpResponse = fetch_json("DELETE", &url, None, loc).await?;
    if let Some(e) = r.error {
        return Err(e);
    }
    Ok(())
}

async fn delete_workspace_dir_via_post(
    path: &str,
    recursive: bool,
    loc: Locale,
) -> Result<(), String> {
    let body = serde_json::to_string(&WorkspaceDirDeletePayload {
        path: path.to_string(),
        delete: true,
        confirm: true,
        recursive,
    })
    .map_err(|e| e.to_string())?;
    let r: WorkspaceDirOpResponse =
        fetch_json_with_body("POST", "/workspace/dir", &body, loc).await?;
    if let Some(e) = r.error {
        return Err(e);
    }
    Ok(())
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

pub async fn fetch_github_repo_context(loc: Locale) -> Result<GithubRepoContextData, String> {
    fetch_json("GET", "/github/repo-context", None, loc).await
}

pub async fn fetch_github_prs(loc: Locale) -> Result<GithubPrsData, String> {
    fetch_json("GET", "/github/prs", None, loc).await
}

pub async fn fetch_github_pr_current_checks(
    loc: Locale,
) -> Result<GithubPrCurrentChecksData, String> {
    fetch_json("GET", "/github/pr/current/checks", None, loc).await
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

fn http_error_detail_from_body(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if let Some(msg) = v
            .get("error")
            .or_else(|| v.get("message"))
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return msg.to_string();
        }
    }
    if trimmed.len() <= 240 {
        trimmed.to_string()
    } else {
        format!("{}…", truncate_to_char_boundary(trimmed, 240))
    }
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
    let status = resp.status();
    let text = JsFuture::from(resp.text().map_err(|e| format!("text: {:?}", e))?)
        .await
        .map_err(|e| format!("read body: {:?}", e))?;
    let s = text
        .as_string()
        .ok_or_else(|| crate::i18n::api_err_body_type(loc).to_string())?;
    if !(200..300).contains(&status) {
        return Err(crate::i18n::api_err_http_status(
            loc,
            status,
            http_error_detail_from_body(&s).as_str(),
        ));
    }
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
    let files = upload_files_multipart_raw(form, loc).await?;
    Ok(files.into_iter().map(|f| f.url).collect())
}

/// `POST /upload` 原始版本：返回完整 `UploadedFileInfo` 列表（供 trait 抽象使用）。
pub async fn upload_files_multipart_raw(
    form: &web_sys::FormData,
    loc: Locale,
) -> Result<Vec<UploadedFileInfo>, String> {
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
    Ok(body.files)
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

/// `POST /chat/branch` 可能返回的错误类型。
#[derive(Debug, Clone)]
pub enum ChatBranchError {
    /// 后端不认识该 `conversation_id`（HTTP 404）。
    NotFound,
    /// revision 冲突（HTTP 409）。
    Conflict,
    /// 其它错误。
    Other(String),
}

impl ChatBranchError {
    fn from_response(resp: &Response, loc: Locale) -> Self {
        match resp.status() {
            404 => ChatBranchError::NotFound,
            409 => ChatBranchError::Conflict,
            _ => ChatBranchError::Other(crate::i18n::api_err_request_failed(loc).to_string()),
        }
    }

    pub fn as_deref(&self) -> &str {
        match self {
            ChatBranchError::NotFound => "会话不存在或已过期",
            ChatBranchError::Conflict => "会话 revision 冲突",
            ChatBranchError::Other(s) => s,
        }
    }
}

impl std::fmt::Display for ChatBranchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_deref())
    }
}

impl std::error::Error for ChatBranchError {}

/// `POST /chat/branch`：服务端按 `before_user_ordinal` 截断持久化会话（须 `conversation_store_sqlite_path` 等已启用）。
/// `GET /conversation/messages`：拉取服务端已持久化会话（与 `conversation_id` + `revision` 对齐）。
pub async fn fetch_conversation_messages(
    conversation_id: &str,
    params: crate::conversation_messages_page::ConversationMessagesFetchParams,
    loc: Locale,
) -> Result<crate::conversation_hydrate::ConversationMessagesResponse, String> {
    let enc = urlencoding::encode(conversation_id);
    let mut url = format!("/conversation/messages?conversation_id={enc}");
    if let Some(limit) = params.limit.filter(|&n| n > 0) {
        url.push_str(&format!("&limit={limit}"));
        if let Some(before) = params.before_index {
            url.push_str(&format!("&before_index={before}"));
        }
    }
    fetch_json("GET", &url, None, loc).await
}

pub async fn post_chat_branch(
    conversation_id: &str,
    before_user_ordinal: u64,
    expected_revision: u64,
    loc: Locale,
) -> Result<u64, ChatBranchError> {
    let body = serde_json::to_string(&ChatBranchBody {
        conversation_id: conversation_id.to_string(),
        before_user_ordinal,
        expected_revision,
    })
    .map_err(|e| ChatBranchError::Other(e.to_string()))?;
    let w = window().ok_or_else(|| ChatBranchError::Other("no window".to_string()))?;
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    let _ = h.set("Content-Type", "application/json");
    init.set_headers(&h);
    init.set_body(&wasm_bindgen::JsValue::from_str(&body));
    let req = Request::new_with_str_and_init("/chat/branch", &init)
        .map_err(|e| ChatBranchError::Other(format!("req: {:?}", e)))?;
    let resp_val = JsFuture::from(w.fetch_with_request(&req))
        .await
        .map_err(|e| ChatBranchError::Other(format!("fetch: {:?}", e)))?;
    let resp: Response = resp_val
        .dyn_into()
        .map_err(|_| ChatBranchError::Other("not Response".to_string()))?;
    if !resp.ok() {
        return Err(ChatBranchError::from_response(&resp, loc));
    }
    let text = JsFuture::from(
        resp.text()
            .map_err(|e| ChatBranchError::Other(format!("text: {:?}", e)))?,
    )
    .await
    .map_err(|e| ChatBranchError::Other(format!("read body: {:?}", e)))?;
    let s = text
        .as_string()
        .ok_or_else(|| ChatBranchError::Other("body not string".to_string()))?;
    let r: ChatBranchResponse =
        serde_json::from_str(&s).map_err(|e| ChatBranchError::Other(e.to_string()))?;
    if !r.ok {
        return Err(ChatBranchError::Other(
            crate::i18n::api_err_branch_failed(loc).to_string(),
        ));
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

#[cfg(test)]
mod tests {
    use super::http_error_detail_from_body;

    #[test]
    fn http_error_detail_truncates_utf8_safely() {
        let body = format!("{}你", "a".repeat(239));
        let detail = http_error_detail_from_body(&body);

        assert!(detail.ends_with('…'));
        assert!(detail.is_char_boundary(detail.len()));
    }
}
