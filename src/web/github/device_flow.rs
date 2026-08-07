//! GitHub OAuth Device Flow：向 GitHub 要码、后台轮询、写入钥匙串账户 `github`。
//!
//! `device_code` 仅留在本模块内存；HTTP 响应不回传 token / device_code。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::user_data::{install_github_cli_token_provider, write_secret_github};
use crate::web::app_state::AppStateHttpCore;

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
}

static DEVICE_SESSION: Mutex<Option<DeviceSession>> = Mutex::const_new(None);

fn oauth_client_id_from_env() -> Option<String> {
    std::env::var("CM_GITHUB_OAUTH_CLIENT_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 优先级：环境变量 **`CM_GITHUB_OAUTH_CLIENT_ID`** → 钥匙串账户 **`github_oauth_client_id`**。
fn resolve_oauth_client_id() -> Option<String> {
    oauth_client_id_from_env().or_else(|| {
        crate::user_data::read_secret_github_oauth_client_id()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    })
}

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

pub(crate) async fn github_oauth_device_start_handler(
    State(http): State<AppStateHttpCore>,
) -> Result<Json<DeviceStartResponse>, (StatusCode, Json<serde_json::Value>)> {
    let Some(client_id) = resolve_oauth_client_id() else {
        return Err(err_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "GITHUB_OAUTH_NOT_CONFIGURED",
            "未配置 GitHub OAuth Client ID：请设置环境变量 CM_GITHUB_OAUTH_CLIENT_ID，或在「设置 → 工具 → GitHub」写入钥匙串",
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

pub(crate) async fn github_oauth_device_status_handler() -> Json<DeviceStatusResponse> {
    let guard = DEVICE_SESSION.lock().await;
    match guard.as_ref() {
        None => Json(DeviceStatusResponse {
            state: DeviceFlowState::Cancelled,
            login: None,
            scopes: None,
            error: Some("无进行中的 Device Flow 会话".into()),
        }),
        Some(s) => Json(DeviceStatusResponse {
            state: s.state,
            login: s.login.clone(),
            scopes: s.scopes.clone(),
            error: s.error.clone(),
        }),
    }
}

pub(crate) async fn github_oauth_device_cancel_handler() -> StatusCode {
    let mut guard = DEVICE_SESSION.lock().await;
    if let Some(s) = guard.as_mut() {
        s.cancel.store(true, Ordering::SeqCst);
        s.state = DeviceFlowState::Cancelled;
    }
    *guard = None;
    StatusCode::NO_CONTENT
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
    if let Err(e) = write_secret_github(token) {
        mark_session(
            cancel,
            DeviceFlowState::Error,
            Some(format!("写入钥匙串失败：{e}")),
            false,
            None,
            None,
        )
        .await;
        return;
    }
    install_github_cli_token_provider();
    mark_session(cancel, DeviceFlowState::Success, None, true, login, scopes).await;
}

/// 处理非 access_token 的 OAuth 错误；返回是否应结束轮询。
async fn handle_oauth_error(cancel: &Arc<AtomicBool>, err: &str, interval: Duration) -> bool {
    match err {
        "authorization_pending" => {
            mark_session(cancel, DeviceFlowState::Pending, None, false, None, None).await;
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
            mark_session(cancel, DeviceFlowState::Error, Some(msg), false, None, None).await;
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
}
