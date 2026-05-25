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
        Err(crate::i18n::api_err_request_failed(loc).to_string())
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
