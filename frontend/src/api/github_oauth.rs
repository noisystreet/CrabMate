//! GitHub OAuth Device Flow 前端 API。

use serde::Deserialize;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

use super::browser::{api_url, auth_headers, window};
use crate::i18n::Locale;

#[derive(Debug, Clone, Deserialize)]
pub struct GithubDeviceStartDto {
    pub user_code: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GithubDeviceStatusDto {
    pub state: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub login: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub scopes: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

pub async fn post_github_oauth_device_start(loc: Locale) -> Result<GithubDeviceStartDto, String> {
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    init.set_headers(&h);
    let req = Request::new_with_str_and_init(&api_url("/github/oauth/device/start"), &init)
        .map_err(|e| format!("request: {e:?}"))?;
    let w = window().ok_or_else(|| crate::i18n::api_err_no_window(loc).to_string())?;
    let resp_val = JsFuture::from(w.fetch_with_request(&req))
        .await
        .map_err(|e| format!("fetch: {e:?}"))?;
    let resp: Response = resp_val
        .dyn_into()
        .map_err(|_| crate::i18n::api_err_response_type(loc))?;
    let text = JsFuture::from(resp.text().map_err(|e| format!("text: {e:?}"))?)
        .await
        .map_err(|e| format!("read body: {e:?}"))?;
    let s = text
        .as_string()
        .ok_or_else(|| crate::i18n::api_err_body_type(loc).to_string())?;
    if !resp.ok() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s)
            && let Some(err) = v.get("error").and_then(|x| x.as_str())
        {
            return Err(err.to_string());
        }
        return Err(crate::i18n::api_err_request_failed(loc).to_string());
    }
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

pub async fn fetch_github_oauth_device_status(
    loc: Locale,
) -> Result<GithubDeviceStatusDto, String> {
    let init = RequestInit::new();
    init.set_method("GET");
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    init.set_headers(&h);
    let req = Request::new_with_str_and_init(&api_url("/github/oauth/device/status"), &init)
        .map_err(|e| format!("request: {e:?}"))?;
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

pub async fn post_github_oauth_device_cancel(loc: Locale) -> Result<(), String> {
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_mode(RequestMode::Cors);
    let h = auth_headers();
    init.set_headers(&h);
    let req = Request::new_with_str_and_init(&api_url("/github/oauth/device/cancel"), &init)
        .map_err(|e| format!("request: {e:?}"))?;
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
    Ok(())
}
