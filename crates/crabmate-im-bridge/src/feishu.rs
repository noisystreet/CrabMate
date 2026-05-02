//! 飞书开放平台 **事件订阅（HTTP Webhook）**：
//! - **明文**：直接解析 JSON。
//! - **加密体**（顶层 **`encrypt`**）：按飞书文档 **AES-256-CBC** 解密后再解析（需配置 **`FEISHU_ENCRYPT_KEY`**）。
//! - `url_verification`：返回 **`{"challenge":"..."}`**。
//! - `im.message.receive_v1`（文本）：默认 **先入队再立即 HTTP 200**（异步 ACK），后台 worker 调 **`POST /chat`** 并回复飞书；可关闭为同步处理（见配置）。
//!
//! 签名校验（可选）：若配置了 **Encrypt Key**，且请求带 **`X-Lark-Signature`** 等头，则按飞书文档
//! `SHA256(timestamp + nonce + encrypt_key + body)` 十六进制小写比对（**原始 HTTP body 字符串**；**URL 校验请求可能无签名头**，此时跳过校验）。
//!
//! 签名校验通过后可选 **防重放**：校验 **`X-Lark-Request-Timestamp`** 与服务器时间偏差，并对 **`X-Lark-Request-Nonce`** 做短期去重（见配置项）。

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
use tokio::sync::{Mutex, mpsc};
use tracing::{error, warn};

use crate::crabmate::{CrabmateClient, CrabmateError};
use crate::feishu_decrypt::{FeishuDecryptError, maybe_decrypt_event_json};

/// 飞书桥接配置（通常由 `crabmate-im-bridge` 二进制从环境变量组装）。
#[derive(Clone)]
pub struct FeishuBridgeConfig {
    pub app_id: String,
    pub app_secret: String,
    /// 事件订阅 **Encrypt Key**（与飞书后台「事件配置」一致；可为空表示不做签名校验）。
    pub encrypt_key: Option<String>,
    /// 为 true 时：在 **encrypt_key 非空** 且请求含签名头时校验；**URL 校验**无签名头时跳过。
    pub verify_signature_when_possible: bool,
    /// 飞书控制台 **Verification Token**（可选）。若设置，则在解密/解析后的 JSON 上校验 **`header.token`**（或顶层 **`token`**）与之相等。
    pub verification_token: Option<String>,
    /// 签名校验通过时：拒绝与当前时间相差超过该秒数的 **`X-Lark-Request-Timestamp`**（`0` 表示不校验时间）。
    pub replay_timestamp_max_skew_secs: i64,
    /// 签名校验通过时：**`X-Lark-Request-Nonce`** 去重窗口（防重放）；`0` 表示不去重 nonce。
    pub nonce_dedup_ttl: Duration,
    /// 群聊（`chat_type == group`）是否仅在有 **@ 本机器人** 时处理（需配置 **`bot_open_id`**）。
    pub group_require_bot_mention: bool,
    /// 本应用机器人在飞书中的 **`open_id`**（开发者后台 / 调试台获取）；与 **`group_require_bot_mention`** 联用。
    pub bot_open_id: Option<String>,
    pub crabmate: Arc<CrabmateClient>,
    /// 幂等：同一 `message_id` 在窗口内忽略（飞书可能重复推送）。
    pub dedup_ttl: Duration,
    /// 为 true 时 **`im.message.receive_v1`** 先入内存队列并 **立即返回 HTTP 200**（飞书异步 ACK）；为 false 时在 HTTP 线程内同步处理完再返回。
    pub async_worker: bool,
    /// 异步队列容量（`try_send` 满时返回 **503** 以便飞书重试）；仅在 **`async_worker`** 为 true 时生效，至少为 **1**。
    pub event_queue_capacity: usize,
}

pub struct FeishuBridgeState {
    cfg: FeishuBridgeConfig,
    http: reqwest::Client,
    token: Mutex<TenantTokenCache>,
    seen_message_ids: DashMap<String, Instant>,
    seen_lark_nonces: DashMap<String, Instant>,
    /// `None`：同步处理；`Some(tx)`：异步 worker 消费。
    event_tx: Option<mpsc::Sender<Value>>,
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

        let (event_tx, event_rx) = if cfg.async_worker && cfg.event_queue_capacity > 0 {
            let cap = cfg.event_queue_capacity.max(1);
            let (tx, rx) = mpsc::channel(cap);
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };

        let state = Arc::new(Self {
            cfg,
            http,
            token: Mutex::new(TenantTokenCache {
                token: String::new(),
                expires_at: 0,
            }),
            seen_message_ids: DashMap::new(),
            seen_lark_nonces: DashMap::new(),
            event_tx,
        });

        if let Some(mut rx) = event_rx {
            let st = Arc::clone(&state);
            tokio::spawn(async move {
                while let Some(envelope) = rx.recv().await {
                    if let Err(e) = handle_im_message_receive(&st, &envelope).await {
                        error!(?e, "async feishu im.message.receive_v1 worker failed");
                    }
                }
            });
        }

        Ok(state)
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

    let signature_verified = match verify_lark_signature_if_needed(&st.cfg, &headers, body_str) {
        Ok(v) => v,
        Err(e) => {
            warn!(?e, "feishu signature verification failed");
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "invalid signature" })),
            )
                .into_response();
        }
    };

    if signature_verified
        && let Err(e) = check_lark_replay_after_signature(
            &st.cfg,
            &st.seen_lark_nonces,
            &headers,
            time_unix_secs(),
        )
    {
        warn!(?e, "feishu replay protection rejected request");
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    let payload_str = match maybe_decrypt_event_json(st.cfg.encrypt_key.as_deref(), body_str) {
        Ok(Some(s)) => s,
        Ok(None) => body_str.to_string(),
        Err(e) => {
            warn!(?e, "feishu decrypt failed");
            let msg = match e {
                FeishuDecryptError::MissingEncryptKey => {
                    "encrypted event requires FEISHU_ENCRYPT_KEY".to_string()
                }
                _ => e.to_string(),
            };
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response();
        }
    };

    let v: Value = match serde_json::from_str(&payload_str) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("invalid json after decrypt: {e}") })),
            )
                .into_response();
        }
    };

    if let Err(msg) = verify_event_verification_token(&st.cfg, &v) {
        warn!(%msg, "feishu verification token mismatch");
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": msg }))).into_response();
    }

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
        Some("im.message.receive_v1") => {
            if let Some(tx) = &st.event_tx {
                match tx.try_send(v) {
                    Ok(()) => {
                        tracing::debug!("feishu im.message.receive_v1 enqueued");
                        (StatusCode::OK, Json(json!({}))).into_response()
                    }
                    Err(e) => {
                        warn!(?e, "feishu event queue full");
                        (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(json!({
                                "error": "event queue full; retry later",
                                "code": "FEISHU_EVENT_QUEUE_FULL"
                            })),
                        )
                            .into_response()
                    }
                }
            } else {
                match handle_im_message_receive(&st, &v).await {
                    Ok(()) => (StatusCode::OK, Json(json!({}))).into_response(),
                    Err(e) => {
                        error!(?e, "handle im.message.receive_v1 failed");
                        (StatusCode::OK, Json(json!({}))).into_response()
                    }
                }
            }
        }
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

/// 返回 **`true`** 表示本次请求完成了 **X-Lark-Signature** 验签（可用于后续防重放）；无签名头或未配置密钥则为 **`false`**。
fn verify_lark_signature_if_needed(
    cfg: &FeishuBridgeConfig,
    headers: &HeaderMap,
    body: &str,
) -> Result<bool, SignatureError> {
    let Some(key) = &cfg.encrypt_key else {
        return Ok(false);
    };
    if !cfg.verify_signature_when_possible {
        return Ok(false);
    }
    let sig = match headers
        .get("X-Lark-Signature")
        .and_then(|h| h.to_str().ok())
    {
        Some(s) if !s.is_empty() => s,
        // URL 校验等场景可能无签名头：不拦截。
        _ => return Ok(false),
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
    Ok(true)
}

#[derive(Debug, thiserror::Error)]
enum ReplayError {
    #[error("X-Lark-Request-Timestamp skew too large")]
    TimestampSkew,
    #[error("duplicate X-Lark-Request-Nonce")]
    DuplicateNonce,
}

fn check_lark_replay_after_signature(
    cfg: &FeishuBridgeConfig,
    nonce_map: &DashMap<String, Instant>,
    headers: &HeaderMap,
    now_secs: i64,
) -> Result<(), ReplayError> {
    if cfg.replay_timestamp_max_skew_secs > 0 {
        let raw = headers
            .get("X-Lark-Request-Timestamp")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        if let Some(ts) = parse_lark_timestamp_secs(raw) {
            let skew = (now_secs - ts).abs();
            if skew > cfg.replay_timestamp_max_skew_secs {
                return Err(ReplayError::TimestampSkew);
            }
        }
    }

    if cfg.nonce_dedup_ttl > Duration::ZERO {
        let nonce = headers
            .get("X-Lark-Request-Nonce")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        if !nonce.is_empty() && is_duplicate(nonce_map, nonce, cfg.nonce_dedup_ttl) {
            return Err(ReplayError::DuplicateNonce);
        }
    }

    Ok(())
}

/// 飞书时间戳多为 **秒** 字符串；若数值过大则按 **毫秒** 解析。
fn parse_lark_timestamp_secs(raw: &str) -> Option<i64> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    let n: i64 = t.parse().ok()?;
    if n > 10_000_000_000 {
        Some(n / 1000)
    } else {
        Some(n)
    }
}

fn verify_event_verification_token(cfg: &FeishuBridgeConfig, v: &Value) -> Result<(), String> {
    let Some(expected) = &cfg.verification_token else {
        return Ok(());
    };
    let exp = expected.trim();
    if exp.is_empty() {
        return Ok(());
    }
    let token = v
        .pointer("/header/token")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("token").and_then(|x| x.as_str()))
        .unwrap_or("");
    if token == exp {
        Ok(())
    } else {
        Err("verification token mismatch".into())
    }
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

    let chat_type = message
        .get("chat_type")
        .and_then(|x| x.as_str())
        .unwrap_or("");

    if st.cfg.group_require_bot_mention && chat_type == "group" {
        let Some(bot_id) = st
            .cfg
            .bot_open_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            warn!(
                "FEISHU_GROUP_REQUIRE_BOT_MENTION=1 but FEISHU_BOT_OPEN_ID empty; skip group message"
            );
            return Ok(());
        };
        if !message_mentions_bot_open_id(&message, bot_id) {
            return Ok(());
        }
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

fn message_mentions_bot_open_id(message: &Value, bot_open_id: &str) -> bool {
    let Some(arr) = message.get("mentions").and_then(|m| m.as_array()) else {
        return false;
    };
    arr.iter().any(|m| {
        m.get("mentioned_type").and_then(|t| t.as_str()) == Some("bot")
            && m.pointer("/id/open_id").and_then(|x| x.as_str()) == Some(bot_open_id)
    })
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
            verification_token: None,
            replay_timestamp_max_skew_secs: 0,
            nonce_dedup_ttl: Duration::ZERO,
            group_require_bot_mention: false,
            bot_open_id: None,
            crabmate: std::sync::Arc::new(
                CrabmateClient::new("http://127.0.0.1:9", "b").expect("client"),
            ),
            dedup_ttl: Duration::from_secs(1),
            async_worker: false,
            event_queue_capacity: 1,
        };
        let headers = HeaderMap::new();
        assert!(!verify_lark_signature_if_needed(&cfg, &headers, "{}").unwrap());
    }

    #[test]
    fn parse_lark_ts_seconds_vs_millis() {
        assert_eq!(parse_lark_timestamp_secs("1600000000"), Some(1_600_000_000));
        assert_eq!(
            parse_lark_timestamp_secs("1600000000000"),
            Some(1_600_000_000)
        );
    }

    #[test]
    fn verification_token_ok() {
        let cfg = FeishuBridgeConfig {
            app_id: "x".into(),
            app_secret: "y".into(),
            encrypt_key: None,
            verify_signature_when_possible: false,
            verification_token: Some("vtok".into()),
            replay_timestamp_max_skew_secs: 0,
            nonce_dedup_ttl: Duration::ZERO,
            group_require_bot_mention: false,
            bot_open_id: None,
            crabmate: std::sync::Arc::new(
                CrabmateClient::new("http://127.0.0.1:9", "b").expect("client"),
            ),
            dedup_ttl: Duration::from_secs(1),
            async_worker: false,
            event_queue_capacity: 1,
        };
        let v = json!({ "header": { "token": "vtok" } });
        assert!(verify_event_verification_token(&cfg, &v).is_ok());
    }

    #[test]
    fn mentions_detect_bot_open_id() {
        let m = json!({
            "mentions": [
                {
                    "mentioned_type": "bot",
                    "id": { "open_id": "ou_bot_1" }
                }
            ]
        });
        assert!(message_mentions_bot_open_id(&m, "ou_bot_1"));
        assert!(!message_mentions_bot_open_id(&m, "ou_other"));
    }
}
