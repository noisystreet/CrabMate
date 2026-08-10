//! GitHub OAuth Device Flow：客户端提供 `client_id`，服务端代要码与轮询；成功后经 Cookie / JSON 交给客户端。
//!
//! `device_code` 仅留在本模块内存；服务端不把 user token 写入钥匙串或磁盘。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::web::app_state::AppStateHttpCore;
use crate::web::github_token_request::{
    GITHUB_TOKEN_COOKIE_NAME, wants_github_token_body_delivery,
};

const GITHUB_DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const GITHUB_ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeviceFlowState {
    Pending,
    SlowDown,
    Success,
    Denied,
    Expired,
    Cancelled,
    Error,
}

struct DeviceSession {
    device_code: String,
    client_id: String,
    interval: Duration,
    expires_at: Instant,
    cancel: Arc<AtomicBool>,
    state: DeviceFlowState,
    login: Option<String>,
    scopes: Option<String>,
    error: Option<String>,
    /// 成功后暂存；status 投递（Cookie / 可选 body）后清空。
    access_token: Option<String>,
}

static DEVICE_SESSION: Mutex<Option<DeviceSession>> = Mutex::const_new(None);

/// OAuth App 可用 `repo` 等；GitHub App Device Flow 通常**省略** scope（靠 App 权限）。
/// 环境变量 **`CM_GITHUB_OAUTH_SCOPES`**：未设置或空 → 不传 `scope`；非空则原样提交（空格分隔）。
fn oauth_scopes_from_env() -> Option<String> {
    std::env::var("CM_GITHUB_OAUTH_SCOPES")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn ensure_verification_uri_complete(verification_uri: &str, user_code: &str) -> String {
    let base = verification_uri.trim();
    let code = user_code.trim();
    if base.is_empty() {
        return format!("https://github.com/login/device?user_code={code}");
    }
    if base.contains("user_code=") {
        return base.to_string();
    }
    let sep = if base.contains('?') { '&' } else { '?' };
    format!("{base}{sep}user_code={code}")
}

fn validate_oauth_client_id(raw: &str) -> Option<String> {
    let id = raw.trim();
    if id.is_empty() || id.len() > 128 {
        return None;
    }
    if !id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return None;
    }
    Some(id.to_string())
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeviceStartRequest {
    pub client_id: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DeviceStartResponse {
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct DeviceStatusResponse {
    pub state: DeviceFlowState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
}

fn err_json(
    status: StatusCode,
    code: &str,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(json!({
            "ok": false,
            "code": code,
            "error": message,
        })),
    )
}

fn form_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn form_body(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", form_encode(k), form_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn origin_matches_host(origin: &str, host: &str) -> bool {
    let origin = origin.trim().trim_end_matches('/');
    let host = host.trim();
    if let Some(rest) = origin.strip_prefix("https://") {
        return rest == host || rest == format!("www.{host}") || host == format!("www.{rest}");
    }
    if let Some(rest) = origin.strip_prefix("http://") {
        return rest == host;
    }
    false
}

fn request_is_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|p| {
            p.split(',')
                .next()
                .is_some_and(|s| s.trim().eq_ignore_ascii_case("https"))
        })
        || headers
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|o| o.trim().starts_with("https://"))
}

fn cookie_secure_flag(headers: &HeaderMap) -> bool {
    if request_is_https(headers) {
        return true;
    }
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let host_only = host.split(':').next().unwrap_or("");
    matches!(host_only, "localhost" | "127.0.0.1" | "::1")
}

fn cookie_samesite(headers: &HeaderMap) -> &'static str {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match (origin, host) {
        (Some(o), Some(h)) if origin_matches_host(o, h) => "Strict",
        (Some(_), _) => "None",
        _ => "Strict",
    }
}

fn set_github_token_cookie_header(token: &str, headers: &HeaderMap) -> Option<HeaderValue> {
    let mut parts = vec![
        format!("{GITHUB_TOKEN_COOKIE_NAME}={}", form_encode(token)),
        "HttpOnly".into(),
        "Path=/".into(),
        format!("SameSite={}", cookie_samesite(headers)),
    ];
    if cookie_secure_flag(headers) || cookie_samesite(headers) == "None" {
        parts.push("Secure".into());
    }
    HeaderValue::from_str(&parts.join("; ")).ok()
}

fn clear_github_token_cookie_header(headers: &HeaderMap) -> Option<HeaderValue> {
    let mut parts = vec![
        format!("{GITHUB_TOKEN_COOKIE_NAME}="),
        "HttpOnly".into(),
        "Path=/".into(),
        "Max-Age=0".into(),
        format!("SameSite={}", cookie_samesite(headers)),
    ];
    if cookie_secure_flag(headers) || cookie_samesite(headers) == "None" {
        parts.push("Secure".into());
    }
    HeaderValue::from_str(&parts.join("; ")).ok()
}

pub(crate) async fn github_oauth_device_start_handler(
    State(http): State<AppStateHttpCore>,
    Json(body): Json<DeviceStartRequest>,
) -> Result<Json<DeviceStartResponse>, (StatusCode, Json<serde_json::Value>)> {
    let Some(client_id) = validate_oauth_client_id(&body.client_id) else {
        return Err(err_json(
            StatusCode::BAD_REQUEST,
            "GITHUB_OAUTH_CLIENT_ID_REQUIRED",
            "请在请求体提供非空的 client_id（GitHub OAuth / App Client ID）",
        ));
    };

    let scopes_opt = oauth_scopes_from_env();
    let mut form_pairs: Vec<(&str, &str)> = vec![("client_id", client_id.as_str())];
    if let Some(ref sc) = scopes_opt {
        form_pairs.push(("scope", sc.as_str()));
    }
    let form = form_body(&form_pairs);
    let resp = http
        .client
        .post(GITHUB_DEVICE_CODE_URL)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .map_err(|e| {
            err_json(
                StatusCode::BAD_GATEWAY,
                "GITHUB_DEVICE_START_FAILED",
                &format!("请求 GitHub device/code 失败：{e}"),
            )
        })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        warn!(
            target: "crabmate",
            %status,
            body_len = body.len(),
            "GitHub device/code 非成功响应"
        );
        return Err(err_json(
            StatusCode::BAD_GATEWAY,
            "GITHUB_DEVICE_START_FAILED",
            &format!("GitHub device/code 返回 HTTP {status}"),
        ));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| {
        err_json(
            StatusCode::BAD_GATEWAY,
            "GITHUB_DEVICE_START_FAILED",
            &format!("解析 device/code 响应失败：{e}"),
        )
    })?;

    let user_code = v
        .get("user_code")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let device_code = v
        .get("device_code")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let verification_uri = v
        .get("verification_uri")
        .and_then(|x| x.as_str())
        .unwrap_or("https://github.com/login/device")
        .trim()
        .to_string();
    let verification_uri_complete = v
        .get("verification_uri_complete")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| ensure_verification_uri_complete(&verification_uri, &user_code));
    let expires_in = v
        .get("expires_in")
        .and_then(|x| x.as_u64())
        .unwrap_or(900)
        .max(60);
    let interval_secs = v
        .get("interval")
        .and_then(|x| x.as_u64())
        .unwrap_or(5)
        .max(1);

    if user_code.is_empty() || device_code.is_empty() {
        return Err(err_json(
            StatusCode::BAD_GATEWAY,
            "GITHUB_DEVICE_START_FAILED",
            "GitHub device/code 响应缺少 user_code 或 device_code",
        ));
    }

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = DEVICE_SESSION.lock().await;
        if let Some(prev) = guard.as_ref() {
            prev.cancel.store(true, Ordering::SeqCst);
        }
        *guard = Some(DeviceSession {
            device_code: device_code.clone(),
            client_id: client_id.clone(),
            interval: Duration::from_secs(interval_secs),
            expires_at: Instant::now() + Duration::from_secs(expires_in),
            cancel: Arc::clone(&cancel),
            state: DeviceFlowState::Pending,
            login: None,
            scopes: None,
            error: None,
            access_token: None,
        });
    }

    let client = http.client.clone();
    tokio::spawn(async move {
        poll_device_token(client, cancel).await;
    });

    Ok(Json(DeviceStartResponse {
        user_code,
        verification_uri,
        verification_uri_complete,
        expires_in,
        interval: interval_secs,
    }))
}

pub(crate) async fn github_oauth_device_status_handler(headers: HeaderMap) -> Response {
    let deliver_body = wants_github_token_body_delivery(&headers);
    let mut guard = DEVICE_SESSION.lock().await;
    let (body, set_cookie) = match guard.as_mut() {
        None => (
            DeviceStatusResponse {
                state: DeviceFlowState::Cancelled,
                login: None,
                scopes: None,
                error: Some("无进行中的 Device Flow 会话".into()),
                access_token: None,
            },
            None,
        ),
        Some(s) => {
            let mut access_token = None;
            let mut set_cookie = None;
            if s.state == DeviceFlowState::Success
                && let Some(token) = s.access_token.take()
            {
                set_cookie = set_github_token_cookie_header(&token, &headers);
                if deliver_body {
                    access_token = Some(token);
                } else if set_cookie.is_none() {
                    // Cookie 头构造失败时把 token 放回会话，避免静默丢凭据。
                    s.access_token = Some(token);
                }
            }
            (
                DeviceStatusResponse {
                    state: s.state,
                    login: s.login.clone(),
                    scopes: s.scopes.clone(),
                    error: s.error.clone(),
                    access_token,
                },
                set_cookie,
            )
        }
    };
    drop(guard);
    let mut res = Json(body).into_response();
    if let Some(cookie) = set_cookie {
        res.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    res
}

pub(crate) async fn github_oauth_device_cancel_handler() -> StatusCode {
    let mut guard = DEVICE_SESSION.lock().await;
    if let Some(s) = guard.as_mut() {
        s.cancel.store(true, Ordering::SeqCst);
        s.state = DeviceFlowState::Cancelled;
        s.access_token = None;
    }
    *guard = None;
    StatusCode::NO_CONTENT
}

/// 清除浏览器 HttpOnly GitHub token Cookie（壳另清本机钥匙串）。
pub(crate) async fn github_oauth_device_logout_handler(headers: HeaderMap) -> Response {
    let mut res = StatusCode::NO_CONTENT.into_response();
    if let Some(cookie) = clear_github_token_cookie_header(&headers) {
        res.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    res
}

fn session_is_ours(s: &DeviceSession, cancel: &Arc<AtomicBool>) -> bool {
    Arc::ptr_eq(&s.cancel, cancel)
}

async fn mark_session(
    cancel: &Arc<AtomicBool>,
    state: DeviceFlowState,
    error: Option<String>,
    clear_device_code: bool,
    login: Option<String>,
    scopes: Option<String>,
    access_token: Option<String>,
) {
    let mut guard = DEVICE_SESSION.lock().await;
    let Some(s) = guard.as_mut() else {
        return;
    };
    if !session_is_ours(s, cancel) {
        return;
    }
    s.state = state;
    s.error = error;
    if clear_device_code {
        s.device_code.clear();
    }
    if login.is_some() {
        s.login = login;
    }
    if scopes.is_some() {
        s.scopes = scopes;
    }
    if access_token.is_some() {
        s.access_token = access_token;
    }
}

async fn read_poll_snapshot(
    cancel: &Arc<AtomicBool>,
) -> Option<(String, String, Duration, Instant)> {
    let guard = DEVICE_SESSION.lock().await;
    let s = guard.as_ref()?;
    if !session_is_ours(s, cancel) {
        return None;
    }
    if Instant::now() >= s.expires_at {
        return None;
    }
    Some((
        s.device_code.clone(),
        s.client_id.clone(),
        s.interval,
        s.expires_at,
    ))
}

async fn apply_access_token(
    client: &reqwest::Client,
    cancel: &Arc<AtomicBool>,
    token: &str,
    scopes: Option<String>,
) {
    let login = fetch_github_login(client, token).await;
    mark_session(
        cancel,
        DeviceFlowState::Success,
        None,
        true,
        login,
        scopes,
        Some(token.to_string()),
    )
    .await;
}

/// 处理非 access_token 的 OAuth 错误；返回是否应结束轮询。
async fn handle_oauth_error(cancel: &Arc<AtomicBool>, err: &str, interval: Duration) -> bool {
    match err {
        "authorization_pending" => {
            mark_session(
                cancel,
                DeviceFlowState::Pending,
                None,
                false,
                None,
                None,
                None,
            )
            .await;
            tokio::time::sleep(interval).await;
            false
        }
        "slow_down" => {
            let sleep_for = {
                let mut guard = DEVICE_SESSION.lock().await;
                if let Some(s) = guard.as_mut()
                    && session_is_ours(s, cancel)
                {
                    s.state = DeviceFlowState::SlowDown;
                    s.interval += Duration::from_secs(5);
                    s.interval
                } else {
                    interval + Duration::from_secs(5)
                }
            };
            tokio::time::sleep(sleep_for).await;
            false
        }
        "access_denied" => {
            mark_session(
                cancel,
                DeviceFlowState::Denied,
                Some("用户拒绝了授权".into()),
                false,
                None,
                None,
                None,
            )
            .await;
            true
        }
        "expired_token" => {
            mark_session(
                cancel,
                DeviceFlowState::Expired,
                Some("授权码已过期，请重新连接".into()),
                false,
                None,
                None,
                None,
            )
            .await;
            true
        }
        other => {
            let msg = if other.is_empty() {
                "换取 access_token 失败".into()
            } else {
                format!("GitHub 错误：{other}")
            };
            mark_session(
                cancel,
                DeviceFlowState::Error,
                Some(msg),
                false,
                None,
                None,
                None,
            )
            .await;
            true
        }
    }
}

async fn poll_device_token(client: reqwest::Client, cancel: Arc<AtomicBool>) {
    loop {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        let Some((device_code, client_id, interval, expires_at)) =
            read_poll_snapshot(&cancel).await
        else {
            let mut guard = DEVICE_SESSION.lock().await;
            if let Some(s) = guard.as_mut()
                && session_is_ours(s, &cancel)
                && Instant::now() >= s.expires_at
            {
                s.state = DeviceFlowState::Expired;
                s.error = Some("授权码已过期，请重新连接".into());
            }
            return;
        };

        if Instant::now() >= expires_at {
            mark_session(
                &cancel,
                DeviceFlowState::Expired,
                Some("授权码已过期，请重新连接".into()),
                false,
                None,
                None,
                None,
            )
            .await;
            return;
        }

        let form = form_body(&[
            ("client_id", client_id.as_str()),
            ("device_code", device_code.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ]);
        let resp = match client
            .post(GITHUB_ACCESS_TOKEN_URL)
            .header("Accept", "application/json")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                debug!(target: "crabmate", error = %e, "GitHub access_token 请求失败，将重试");
                tokio::time::sleep(interval).await;
                continue;
            }
        };
        let v: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(_) => {
                tokio::time::sleep(interval).await;
                continue;
            }
        };

        if let Some(token) = v
            .get("access_token")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let scopes = v
                .get("scope")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            apply_access_token(&client, &cancel, token, scopes).await;
            return;
        }

        let err = v.get("error").and_then(|x| x.as_str()).unwrap_or("");
        if handle_oauth_error(&cancel, err, interval).await {
            return;
        }
    }
}

async fn fetch_github_login(client: &reqwest::Client, token: &str) -> Option<String> {
    let resp = client
        .get("https://api.github.com/user")
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "crabmate")
        .bearer_auth(token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    v.get("login")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_uri_appends_user_code() {
        assert_eq!(
            ensure_verification_uri_complete("https://github.com/login/device", "ABCD-1234"),
            "https://github.com/login/device?user_code=ABCD-1234"
        );
        assert!(
            ensure_verification_uri_complete(
                "https://github.com/login/device?user_code=ABCD-1234",
                "ZZZZ"
            )
            .contains("ABCD-1234")
        );
    }

    #[test]
    fn validate_client_id_accepts_github_shapes() {
        assert_eq!(
            validate_oauth_client_id("  Iv1.abcDEF12  ").as_deref(),
            Some("Iv1.abcDEF12")
        );
        assert!(validate_oauth_client_id("").is_none());
        assert!(validate_oauth_client_id("bad id").is_none());
    }

    #[test]
    fn origin_host_match_detects_same_site() {
        assert!(origin_matches_host(
            "http://127.0.0.1:8080",
            "127.0.0.1:8080"
        ));
        assert!(!origin_matches_host(
            "https://ui.example.com",
            "api.example.com"
        ));
    }
}
