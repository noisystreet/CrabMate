//! Web 会话存储切换（`POST /config/session/conversation-store`）。

use serde::Deserialize;
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

use crate::i18n::Locale;

use super::browser::{auth_headers, window};

/// `POST /config/session/conversation-store` 成功体。
#[derive(Debug, Clone, Deserialize)]
pub struct SessionConversationStoreResponse {
    pub ok: bool,
    pub message: String,
}

async fn session_store_post_json_value(body: &str, loc: Locale) -> Result<(u16, Value), String> {
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    let _ = h.set("Content-Type", "application/json");
    init.set_headers(&h);
    init.set_body(&JsValue::from_str(body));
    let req = Request::new_with_str_and_init("/config/session/conversation-store", &init)
        .map_err(|e| format!("request: {:?}", e))?;
    let w = window().ok_or_else(|| crate::i18n::api_err_no_window(loc).to_string())?;
    let resp_val = JsFuture::from(w.fetch_with_request(&req))
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
    let v: Value =
        serde_json::from_str(&s).map_err(|_| crate::i18n::api_err_request_failed(loc))?;
    Ok((status, v))
}

fn session_store_error_message(v: &Value, status: u16) -> String {
    v.get("message")
        .and_then(|x| x.as_str())
        .map(std::string::ToString::to_string)
        .or_else(|| {
            v.get("error")
                .and_then(|x| x.as_str())
                .map(std::string::ToString::to_string)
        })
        .unwrap_or_else(|| format!("HTTP {status}"))
}

pub async fn post_session_conversation_store(
    sqlite: bool,
    loc: Locale,
) -> Result<SessionConversationStoreResponse, String> {
    let body = serde_json::to_string(&serde_json::json!({ "sqlite": sqlite }))
        .map_err(|e| e.to_string())?;
    let (status, v) = session_store_post_json_value(&body, loc).await?;
    if !(200..300).contains(&status) {
        return Err(session_store_error_message(&v, status));
    }
    let r: SessionConversationStoreResponse =
        serde_json::from_value(v).map_err(|e| e.to_string())?;
    if !r.ok {
        return Err(r.message);
    }
    Ok(r)
}
