//! 飞书开放平台 **事件订阅（HTTP Webhook）** MVP：
//! - `url_verification`：原样返回 `challenge`（Encrypt Key 未启用时的明文校验；加密模式见飞书文档，本 MVP 以明文校验为主）。
//! - `im.message.receive_v1`：解析文本 → **`POST /chat`** → [回复消息](https://open.feishu.cn/document/server-docs/im-v1/message/reply)。
//!
//! 签名校验（可选）：若配置了 **Encrypt Key**，且请求带 **`X-Lark-Signature`** 等头，则按飞书文档
//! `SHA256(timestamp + nonce + encrypt_key + body)` 十六进制小写比对（**URL 校验请求可能无签名头**，此时跳过校验）。

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use dashmap::DashMap;
use hex::FromHex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;
use tracing::{error, warn};

use crate::crabmate::{CrabmateClient, CrabmateError};

/// 飞书桥接配置（通常由 `crabmate-im-bridge` 二进制从环境变量组装）。
#[derive(Clone)]
pub struct FeishuBridgeConfig {
    pub app_id: String,
    pub app_secret: String,
    /// 事件订阅 **Encrypt Key**（与飞书后台「事件配置」一致；可为空表示不做签名校验）。
    pub encrypt_key: Option<String>,
    /// 为 true 时：在 **encrypt_key 非空** 且请求含签名头时校验；**URL 校验**无签名头时跳过。
    pub verify_signature_when_possible: bool,
    pub crabmate: Arc<CrabmateClient>,
    /// 幂等：同一 `message_id` 在窗口内忽略（飞书可能重复推送）。
    pub dedup_ttl: Duration,
}

pub struct FeishuBridgeState {
    cfg: FeishuBridgeConfig,
    http: reqwest::Client,
    token: Mutex<TenantTokenCache>,
    seen_message_ids: DashMap<String, Instant>,
}

struct TenantTokenCache {
    token: String,
    /// 绝对 Unix 秒，预留 120s 刷新余量。
    expires_at: i64,
}

impl FeishuBridgeState {
    pub fn try_new(cfg: FeishuBridgeConfig) -> Result<Arc<Self>, reqwest::Error> {
        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .timeout(Duration::from_secs(120))
            .build()?;
        Ok(Arc::new(Self {
            cfg,
            http,
            token: Mutex::new(TenantTokenCache {
                token: String::new(),
                expires_at: 0,
            }),
            seen_message_ids: DashMap::new(),
        }))
    }
}

pub fn build_router(state: Arc<FeishuBridgeState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/feishu/events", post(feishu_events))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn feishu_events(
    State(st): State<Arc<FeishuBridgeState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let body_str = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "body is not valid UTF-8" })),
            )
                .into_response();
        }
    };

    if let Err(e) = verify_lark_signature_if_needed(&st.cfg, &headers, body_str) {
        warn!(?e, "feishu signature verification failed");
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid signature" })),
        )
            .into_response();
    }

    let v: Value = match serde_json::from_str(body_str) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("invalid json: {e}") })),
            )
                .into_response();
        }
    };

    // URL 校验（常见两种载荷）
    if let Some(ch) = url_verification_challenge(&v) {
        return Json(json!({ "challenge": ch })).into_response();
    }

    // 业务事件（schema 2.0：header.event_type + event）
    let event_type = v
        .pointer("/header/event_type")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("type").and_then(|x| x.as_str()));

    match event_type {
        Some("im.message.receive_v1") => match handle_im_message_receive(&st, &v).await {
            Ok(()) => (StatusCode::OK, Json(json!({}))).into_response(),
            Err(e) => {
                error!(?e, "handle im.message.receive_v1 failed");
                (StatusCode::OK, Json(json!({}))).into_response()
            }
        },
        Some(other) => {
            warn!(event_type = other, "ignored feishu event type");
            (StatusCode::OK, Json(json!({}))).into_response()
        }
        None => {
            warn!("missing event type in feishu payload");
            (StatusCode::OK, Json(json!({}))).into_response()
        }
    }
}

fn url_verification_challenge(v: &Value) -> Option<String> {
    if v.get("type").and_then(|t| t.as_str()) == Some("url_verification") {
        return v
            .get("challenge")
            .and_then(|c| c.as_str())
            .map(str::to_string);
    }
    if v.pointer("/header/event_type").and_then(|t| t.as_str()) == Some("url_verification") {
        return v
            .pointer("/event/challenge")
            .or_else(|| v.get("challenge"))
            .and_then(|c| c.as_str())
            .map(str::to_string);
    }
    if let Some(c) = v.get("challenge").and_then(|c| c.as_str())
        && (v.get("token").is_some() || v.pointer("/header/token").is_some())
    {
        return Some(c.to_string());
    }
    None
}

#[derive(Debug, thiserror::Error)]
enum SignatureError {
    #[error("computed signature mismatch")]
    Mismatch,
}

fn verify_lark_signature_if_needed(
    cfg: &FeishuBridgeConfig,
    headers: &HeaderMap,
    body: &str,
) -> Result<(), SignatureError> {
    let Some(key) = &cfg.encrypt_key else {
        return Ok(());
    };
    if !cfg.verify_signature_when_possible {
        return Ok(());
    }
    let sig = match headers
        .get("X-Lark-Signature")
        .and_then(|h| h.to_str().ok())
    {
        Some(s) if !s.is_empty() => s,
        // URL 校验等场景可能无签名头：不拦截。
        _ => return Ok(()),
    };
    let ts = headers
        .get("X-Lark-Request-Timestamp")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let nonce = headers
        .get("X-Lark-Request-Nonce")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let mut hasher = Sha256::new();
    hasher.update(ts.as_bytes());
    hasher.update(nonce.as_bytes());
    hasher.update(key.as_bytes());
    hasher.update(body.as_bytes());
    let out = hasher.finalize();
    let expect = hex::encode(out);
    if !constant_time_eq_hex(&expect, sig) {
        return Err(SignatureError::Mismatch);
    }
    Ok(())
}

fn constant_time_eq_hex(a: &str, b: &str) -> bool {
    let a = a.to_ascii_lowercase();
    let b = b.to_ascii_lowercase();
    let Ok(ab) = Vec::from_hex(a.as_str()) else {
        return false;
    };
    let Ok(bb) = Vec::from_hex(b.as_str()) else {
        return false;
    };
    ab.as_slice().ct_eq(bb.as_slice()).into()
}

async fn handle_im_message_receive(
    st: &FeishuBridgeState,
    envelope: &Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let sender_type = envelope
        .pointer("/event/sender/sender_type")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if sender_type != "user" {
        return Ok(());
    }

    let message = envelope
        .pointer("/event/message")
        .cloned()
        .unwrap_or(json!({}));
    let message_id = message
        .get("message_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if message_id.is_empty() {
        return Ok(());
    }

    if is_duplicate(&st.seen_message_ids, &message_id, st.cfg.dedup_ttl) {
        return Ok(());
    }

    let chat_id = message
        .get("chat_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if chat_id.is_empty() {
        return Ok(());
    }

    let msg_type = message
        .get("message_type")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if msg_type != "text" {
        let _ = reply_text_message(st, &message_id, "当前 MVP 仅支持文本消息。").await;
        return Ok(());
    }

    let content_raw = message
        .get("content")
        .and_then(|x| x.as_str())
        .unwrap_or("{}");
    let text = parse_text_content(content_raw).unwrap_or_default();
    let text = strip_feishu_mention_placeholders(&text);
    let text = text.trim();
    if text.is_empty() {
        return Ok(());
    }

    let conv = format!("feishu:{chat_id}");
    let reply = match st.cfg.crabmate.chat_plain(text, Some(&conv)).await {
        Ok(r) => r.reply,
        Err(CrabmateError::HttpStatus {
            status,
            body_preview,
        }) => {
            warn!(status, %body_preview, "CrabMate /chat error");
            format!("（CrabMate 返回 HTTP {status}，请检查服务与密钥。）")
        }
        Err(e) => {
            warn!(?e, "CrabMate /chat request failed");
            format!("（调用 CrabMate 失败：{e}）")
        }
    };

    let clipped = clip_reply_for_feishu(&reply, 18_000);
    reply_text_message(st, &message_id, &clipped).await?;
    Ok(())
}

fn parse_text_content(content_json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(content_json).ok()?;
    v.get("text").and_then(|t| t.as_str()).map(str::to_string)
}

fn strip_feishu_mention_placeholders(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'@' && i + 7 <= bytes.len() && &bytes[i..i + 7] == b"@_user_" {
            let rest = &s[i + 7..];
            let end = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            i += 7 + end;
            continue;
        }
        let ch = s[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out.trim().to_string()
}

fn clip_reply_for_feishu(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let take = max_chars.saturating_sub(8);
    s.chars().take(take).collect::<String>() + "\n…（已截断）"
}

fn is_duplicate(map: &DashMap<String, Instant>, id: &str, ttl: Duration) -> bool {
    let now = Instant::now();
    if let Some(v) = map.get(id)
        && now.duration_since(*v) < ttl
    {
        return true;
    }
    map.insert(id.to_string(), now);
    // 粗清理：条目过多时清空（MVP；生产可换 TTL 队列）
    if map.len() > 50_000 {
        map.clear();
    }
    false
}

async fn reply_text_message(
    st: &FeishuBridgeState,
    message_id: &str,
    text: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let token = get_tenant_access_token(st).await?;
    let url = format!("https://open.feishu.cn/open-apis/im/v1/messages/{message_id}/reply");
    let content = serde_json::to_string(&json!({ "text": text }))?;
    let body = json!({
        "content": content,
        "msg_type": "text",
        "uuid": message_id,
    });
    let resp = st
        .http
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json; charset=utf-8")
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        let preview = String::from_utf8_lossy(&bytes).trim().to_string();
        warn!(%status, %preview, "feishu reply API http error");
        return Ok(());
    }
    let v: Value = serde_json::from_slice(&bytes)?;
    let code = v.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        warn!(%code, body=%String::from_utf8_lossy(&bytes), "feishu reply API business error");
    }
    Ok(())
}

async fn get_tenant_access_token(
    st: &FeishuBridgeState,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let now = time_unix_secs();
    let mut guard = st.token.lock().await;
    if !guard.token.is_empty() && guard.expires_at.saturating_sub(now) > 120 {
        return Ok(guard.token.clone());
    }

    let url = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
    let body = json!({
        "app_id": st.cfg.app_id,
        "app_secret": st.cfg.app_secret,
    });
    let resp = st.http.post(url).json(&body).send().await?;
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        return Err(format!("token http {}: {}", status, String::from_utf8_lossy(&bytes)).into());
    }
    let v: Value = serde_json::from_slice(&bytes)?;
    let code = v.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        return Err(format!("token api code={code}: {}", String::from_utf8_lossy(&bytes)).into());
    }
    let token = v
        .get("tenant_access_token")
        .and_then(|t| t.as_str())
        .ok_or("missing tenant_access_token")?
        .to_string();
    let expire = v.get("expire").and_then(|e| e.as_i64()).unwrap_or(7200);
    guard.token = token.clone();
    guard.expires_at = now + expire;
    Ok(token)
}

fn time_unix_secs() -> i64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn url_verification_plain_challenge() {
        let v = json!({
            "type": "url_verification",
            "challenge": "abc123"
        });
        assert_eq!(url_verification_challenge(&v), Some("abc123".into()));
    }

    #[test]
    fn signature_skipped_when_no_header() {
        let cfg = FeishuBridgeConfig {
            app_id: "x".into(),
            app_secret: "y".into(),
            encrypt_key: Some("ek".into()),
            verify_signature_when_possible: true,
            crabmate: std::sync::Arc::new(
                CrabmateClient::new("http://127.0.0.1:9", "b").expect("client"),
            ),
            dedup_ttl: Duration::from_secs(1),
        };
        let headers = HeaderMap::new();
        assert!(verify_lark_signature_if_needed(&cfg, &headers, "{}").is_ok());
    }
}
