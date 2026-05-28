//! `GET` / `PUT` / `POST` **`/user-data/*`**（本机用户数据目录，见 `docs/design/user_data_dir.md`）。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

use crate::i18n::Locale;
use crate::storage::ChatSession;

use super::browser::{auth_headers, window};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserPrefsDto {
    #[serde(default)]
    pub last_workspace_root: Option<String>,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub side_panel_view: Option<String>,
    #[serde(default)]
    pub side_width: Option<f64>,
    #[serde(default)]
    pub editor_layout_mode: Option<bool>,
    #[serde(default)]
    pub timeline_panel_expanded: Option<bool>,
    #[serde(default)]
    pub sidebar_rail_collapsed: Option<bool>,
    #[serde(default)]
    pub session_ui_font: Option<String>,
    #[serde(default)]
    pub session_chat_font: Option<String>,
    #[serde(default)]
    pub ide_editor_font: Option<String>,
    #[serde(default)]
    pub ide_editor_font_size: Option<u32>,
    #[serde(default)]
    pub ide_editor_line_numbers: Option<bool>,
    #[serde(default)]
    pub ide_editor_word_wrap: Option<bool>,
    #[serde(default)]
    pub ide_editor_tab_size: Option<u32>,
    #[serde(default)]
    pub bg_decor: Option<bool>,
    #[serde(default)]
    pub status_bar_visible: Option<bool>,
    #[serde(default)]
    pub cm_role: Option<String>,
    #[serde(default)]
    pub disable_readonly_tool_ttl_cache: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LlmEndpointOverrideDto {
    #[serde(default)]
    pub api_base: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub temperature: Option<String>,
    #[serde(default)]
    pub llm_context_tokens: Option<String>,
    #[serde(default)]
    pub llm_thinking_mode: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LlmOverridesDto {
    #[serde(default)]
    pub client_llm: LlmEndpointOverrideDto,
    #[serde(default)]
    pub executor_llm: LlmEndpointOverrideDto,
    #[serde(default)]
    pub execution_mode: Option<String>,
    #[serde(default)]
    pub saved_models: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct WebSessionsDto {
    #[serde(default)]
    sessions: Value,
    #[serde(default)]
    active_session_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct PutSessionsBody {
    sessions: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_session_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SecretSlotStatusDto {
    pub set: bool,
    #[serde(default)]
    #[allow(dead_code)]
    pub suffix: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SecretsStatusDto {
    #[serde(default)]
    pub client_llm: SecretSlotStatusDto,
    #[serde(default)]
    pub executor_llm: SecretSlotStatusDto,
    #[serde(default)]
    #[allow(dead_code)]
    pub web_api_bearer: SecretSlotStatusDto,
}

pub async fn fetch_secrets_status(loc: Locale) -> Result<SecretsStatusDto, String> {
    fetch_json("GET", "/user-data/secrets/status", loc).await
}

async fn fetch_json<T: for<'de> Deserialize<'de>>(
    method: &str,
    url: &str,
    loc: Locale,
) -> Result<T, String> {
    let init = RequestInit::new();
    init.set_method(method);
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    init.set_headers(&h);
    let req = Request::new_with_str_and_init(url, &init).map_err(|e| format!("request: {e:?}"))?;
    let w = window().ok_or_else(|| crate::i18n::api_err_no_window(loc).to_string())?;
    let resp_val = JsFuture::from(w.fetch_with_request(&req))
        .await
        .map_err(|e| format!("fetch: {e:?}"))?;
    let resp: Response = resp_val
        .dyn_into()
        .map_err(|_| crate::i18n::api_err_response_type(loc))?;
    if !resp.ok() {
        return Err(crate::i18n::api_err_request_failed(loc).to_string());
    }
    let text = JsFuture::from(resp.text().map_err(|e| format!("text: {e:?}"))?)
        .await
        .map_err(|e| format!("read body: {e:?}"))?;
    let s = text
        .as_string()
        .ok_or_else(|| crate::i18n::api_err_body_type(loc).to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

async fn put_json_no_content(url: &str, body: &str, loc: Locale) -> Result<(), String> {
    let init = RequestInit::new();
    init.set_method("PUT");
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    let _ = h.set("Content-Type", "application/json");
    init.set_headers(&h);
    init.set_body(&wasm_bindgen::JsValue::from_str(body));
    let req = Request::new_with_str_and_init(url, &init).map_err(|e| format!("request: {e:?}"))?;
    let w = window().ok_or_else(|| crate::i18n::api_err_no_window(loc).to_string())?;
    let resp_val = JsFuture::from(w.fetch_with_request(&req))
        .await
        .map_err(|e| format!("fetch: {e:?}"))?;
    let resp: Response = resp_val
        .dyn_into()
        .map_err(|_| crate::i18n::api_err_response_type(loc))?;
    if resp.ok() {
        Ok(())
    } else {
        let status = resp.status();
        let detail = JsFuture::from(resp.text().map_err(|e| format!("text: {e:?}"))?)
            .await
            .ok()
            .and_then(|t| t.as_string())
            .unwrap_or_default();
        if detail.trim().is_empty() {
            Err(format!(
                "{} ({status})",
                crate::i18n::api_err_request_failed(loc)
            ))
        } else {
            Err(format!(
                "{} ({status}): {}",
                crate::i18n::api_err_request_failed(loc),
                detail.trim()
            ))
        }
    }
}

pub async fn fetch_user_data_prefs(loc: Locale) -> Result<UserPrefsDto, String> {
    fetch_json("GET", "/user-data/prefs", loc).await
}

pub async fn put_user_data_prefs(prefs: &UserPrefsDto, loc: Locale) -> Result<(), String> {
    let body = serde_json::to_string(prefs).map_err(|e| e.to_string())?;
    put_json_no_content("/user-data/prefs", &body, loc).await
}

pub async fn fetch_llm_overrides(loc: Locale) -> Result<LlmOverridesDto, String> {
    fetch_json("GET", "/user-data/llm-overrides", loc).await
}

pub async fn put_llm_overrides(file: &LlmOverridesDto, loc: Locale) -> Result<(), String> {
    let body = serde_json::to_string(file).map_err(|e| e.to_string())?;
    put_json_no_content("/user-data/llm-overrides", &body, loc).await
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpServerEntryDto {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub has_command: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub created_at_ms: i64,
    #[serde(default)]
    pub updated_at_ms: i64,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpServersFileDto {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default = "default_true")]
    pub global_enabled: bool,
    #[serde(default = "default_mcp_timeout")]
    pub tool_timeout_secs: u64,
    #[serde(default)]
    pub servers: Vec<McpServerEntryDto>,
}

fn default_mcp_timeout() -> u64 {
    60
}

/// API 反序列化用；部分字段供后续 UI 展示远端工具列表。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct McpRemoteToolSummaryDto {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct McpServerStatusEntryDto {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub enabled: bool,
    pub connected: bool,
    #[serde(default)]
    pub openai_tool_names: Vec<String>,
    #[serde(default)]
    pub remote_tools: Vec<McpRemoteToolSummaryDto>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct McpServersStatusDto {
    pub global_enabled: bool,
    pub tool_timeout_secs: u64,
    #[serde(default)]
    pub servers: Vec<McpServerStatusEntryDto>,
}

pub async fn fetch_mcp_servers(loc: Locale) -> Result<McpServersFileDto, String> {
    fetch_json("GET", "/user-data/mcp-servers", loc).await
}

pub async fn put_mcp_servers(file: &McpServersFileDto, loc: Locale) -> Result<(), String> {
    let body = serde_json::to_string(file).map_err(|e| e.to_string())?;
    put_json_no_content("/user-data/mcp-servers", &body, loc).await
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct McpServersImportResponseDto {
    pub file: McpServersFileDto,
    pub imported_count: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub skipped_remote: Vec<String>,
}

pub async fn post_mcp_servers_import(
    json_text: &str,
    loc: Locale,
) -> Result<McpServersImportResponseDto, String> {
    let trimmed = json_text.trim();
    if trimmed.is_empty() {
        return Err("JSON 为空".to_string());
    }
    let _: Value = serde_json::from_str(trimmed).map_err(|e| format!("JSON 解析失败: {e}"))?;
    post_json_body("/user-data/mcp-servers/import", trimmed, loc).await
}

async fn post_json_body<T: for<'de> Deserialize<'de>>(
    url: &str,
    body: &str,
    loc: Locale,
) -> Result<T, String> {
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    let _ = h.set("Content-Type", "application/json");
    init.set_headers(&h);
    init.set_body(&wasm_bindgen::JsValue::from_str(body));
    let req = Request::new_with_str_and_init(url, &init).map_err(|e| format!("request: {e:?}"))?;
    let w = window().ok_or_else(|| crate::i18n::api_err_no_window(loc).to_string())?;
    let resp_val = JsFuture::from(w.fetch_with_request(&req))
        .await
        .map_err(|e| format!("fetch: {e:?}"))?;
    let resp: Response = resp_val
        .dyn_into()
        .map_err(|_| crate::i18n::api_err_response_type(loc))?;
    if !resp.ok() {
        return Err(crate::i18n::api_err_request_failed(loc).to_string());
    }
    let text = JsFuture::from(resp.text().map_err(|e| format!("text: {e:?}"))?)
        .await
        .map_err(|e| format!("read body: {e:?}"))?;
    let s = text
        .as_string()
        .ok_or_else(|| crate::i18n::api_err_body_type(loc).to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

pub async fn fetch_mcp_servers_status(loc: Locale) -> Result<McpServersStatusDto, String> {
    fetch_json("GET", "/user-data/mcp-servers/status", loc).await
}

async fn post_json<T: for<'de> Deserialize<'de>>(url: &str, loc: Locale) -> Result<T, String> {
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    init.set_headers(&h);
    let req = Request::new_with_str_and_init(url, &init).map_err(|e| format!("request: {e:?}"))?;
    let w = window().ok_or_else(|| crate::i18n::api_err_no_window(loc).to_string())?;
    let resp_val = JsFuture::from(w.fetch_with_request(&req))
        .await
        .map_err(|e| format!("fetch: {e:?}"))?;
    let resp: Response = resp_val
        .dyn_into()
        .map_err(|_| crate::i18n::api_err_response_type(loc))?;
    if !resp.ok() {
        return Err(crate::i18n::api_err_request_failed(loc).to_string());
    }
    let text = JsFuture::from(resp.text().map_err(|e| format!("text: {e:?}"))?)
        .await
        .map_err(|e| format!("read body: {e:?}"))?;
    let s = text
        .as_string()
        .ok_or_else(|| crate::i18n::api_err_body_type(loc).to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

pub async fn post_mcp_server_probe(
    server_id: &str,
    loc: Locale,
) -> Result<McpServerStatusEntryDto, String> {
    post_json(&format!("/user-data/mcp-servers/{server_id}/probe"), loc).await
}

pub async fn post_mcp_servers_probe_all(
    loc: Locale,
) -> Result<Vec<McpServerStatusEntryDto>, String> {
    post_json("/user-data/mcp-servers/probe-all", loc).await
}

pub async fn put_secret_executor_llm(api_key: &str, loc: Locale) -> Result<(), String> {
    let body = serde_json::json!({ "api_key": api_key }).to_string();
    put_json_no_content("/user-data/secrets/executor-llm", &body, loc).await
}

pub async fn put_secret_client_llm(api_key: &str, loc: Locale) -> Result<(), String> {
    let body = serde_json::json!({ "api_key": api_key }).to_string();
    put_json_no_content("/user-data/secrets/client-llm", &body, loc).await
}

pub async fn fetch_current_web_sessions(
    loc: Locale,
) -> Result<(Vec<ChatSession>, Option<String>), String> {
    let dto: WebSessionsDto =
        fetch_json("GET", "/user-data/workspaces/current/sessions", loc).await?;
    let sessions: Vec<ChatSession> = match dto.sessions {
        Value::Array(arr) => {
            let mut out = Vec::new();
            for v in arr {
                if let Ok(s) = serde_json::from_value::<ChatSession>(v) {
                    out.push(s);
                }
            }
            out
        }
        _ => Vec::new(),
    };
    Ok((sessions, dto.active_session_id))
}

pub async fn put_current_web_sessions(
    sessions: &[ChatSession],
    active_id: Option<&str>,
    loc: Locale,
) -> Result<(), String> {
    let sessions_val = serde_json::to_value(sessions).map_err(|e| e.to_string())?;
    let body = PutSessionsBody {
        sessions: sessions_val,
        active_session_id: active_id.map(str::to_string),
    };
    let json = serde_json::to_string(&body).map_err(|e| e.to_string())?;
    put_json_no_content("/user-data/workspaces/current/sessions", &json, loc).await
}
