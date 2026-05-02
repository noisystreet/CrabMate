use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use dashmap::DashMap;
use futures_util::StreamExt;
use hex::FromHex;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::sleep;
use tracing::{error, warn};

use crate::crabmate::{CrabmateClient, CrabmateError};
use crate::feishu_decrypt::{FeishuDecryptError, maybe_decrypt_event_json};
use crate::feishu_event_queue::FeishuImEventSqliteQueue;
use crate::feishu_message_content::incoming_content_as_user_text;
use crate::feishu_tool_card;
use crate::feishu_workspace::expand_workspace_root_template;
use crate::sse_consumer::{
    StreamAccum, dispatch_sse_event_block_collect, take_complete_sse_blocks,
};

/// 飞书侧敏感工具审批策略（与 CrabMate **`POST /chat/stream`** 的 **`approval_session_id`** 配合）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeishuToolApprovalMode {
    /// 不传 **`approval_session_id`**，敏感工具按服务端默认（通常等同拒绝或需 Web）。
    DenyAll,
    /// 传 **`approval_session_id`**，收到审批帧时**自动** `allow_once`（仅可信环境）。
    DefaultAllowOnce,
    /// 传 **`approval_session_id`**；收到审批后通过 **`POST /feishu/tool-decision`**（须密钥）提交决策，或超时视为 **`deny`**。
    WaitHttp,
    /// 传 **`approval_session_id`**；用户在下一条飞书消息中发送 **`!允许一次`** / **`!永久允许`** / **`!拒绝`**（同一会话内）。
    WaitMessage,
}

/// 等待人工 **`POST /feishu/tool-decision`** 的挂起项。
struct PendingToolDecision {
    reply_tx: oneshot::Sender<String>,
}

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
    /// 将 **`interactive` / 未知类型** 等 `content` 序列化为摘要时的最大字符数（防止超大 JSON 撑爆模型）。
    pub max_message_content_json_chars: usize,
    /// 为 true 时 **`im.message.receive_v1`** 先入内存队列并 **立即返回 HTTP 200**（飞书异步 ACK）；为 false 时在 HTTP 线程内同步处理完再返回。
    pub async_worker: bool,
    /// 异步队列容量（`try_send` 满时返回 **503** 以便飞书重试）；仅在 **`async_worker`** 为 true 时生效，至少为 **1**。
    pub event_queue_capacity: usize,
    /// 若非空：每通会话在调用 CrabMate 前 **`POST /workspace`**；支持 **`{chat_id}`** 占位（飞书 `message.chat_id`）。
    pub workspace_root_template: Option<String>,
    /// 飞书侧工具审批模式（见 [`FeishuToolApprovalMode`]）。
    pub tool_approval_mode: FeishuToolApprovalMode,
    /// **`FEISHU_TOOL_DECISION_SECRET`**：保护 **`POST /feishu/tool-decision`**；**`WaitHttp`** 模式下必填。
    pub tool_decision_secret: Option<String>,
    /// **`WaitHttp`** 下等待人工决策的最长秒数（超时按 **`deny`** 提交）；至少 **5**。
    pub tool_decision_timeout_secs: u64,
    /// 为 true 时：流式处理中**不**把每条 SSE 进度（工具调用、时间线等）逐条发到飞书，仅保留开场提示与结束结果卡片（适合长任务、省 QPS）。
    pub quiet_sse_status: bool,
    /// 结束时的结果卡片内「助手回复」摘要最大字符数（飞书卡片体不宜过大）；至少 **200**。
    pub result_card_max_body_chars: usize,
    /// 为 true 时：开场发送可 **PATCH** 的占位交互卡片，结束时用同一 **`message_id`** 原地更新为结果摘要（失败则回退为独立回复卡片/文本）。
    pub in_place_progress_card: bool,
    /// 若非空且 **`async_worker`**：使用 **SQLite 持久化队列**（与内存 `mpsc` 互斥，本字段优先）。
    pub event_queue_sqlite_path: Option<String>,
    /// SQLite 队列项处理失败后的最大重试次数（不含首次）；至少 **1**。
    pub sqlite_queue_max_retries: u32,
    /// SQLite worker 空闲轮询间隔（毫秒）；至少 **50**。
    pub sqlite_queue_poll_ms: u64,
    /// SQLite 认领租约时长（秒），进程崩溃后过期可被重新认领；至少 **30**。
    pub sqlite_queue_lease_secs: i64,
}

/// `FeishuBridgeState::try_new` 失败原因（HTTP 客户端或 SQLite 队列）。
#[derive(Debug, thiserror::Error)]
pub enum FeishuBridgeInitError {
    #[error("http client: {0}")]
    Http(#[from] reqwest::Error),
    #[error("sqlite queue: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct FeishuBridgeState {
    cfg: FeishuBridgeConfig,
    http: reqwest::Client,
    token: Mutex<TenantTokenCache>,
    seen_message_ids: DashMap<String, Instant>,
    seen_lark_nonces: DashMap<String, Instant>,
    /// `None`：同步处理；`Some(tx)`：异步 worker 消费。
    event_tx: Option<mpsc::Sender<Value>>,
    /// 与 **`event_tx`** 互斥：持久化 **`im.message.receive_v1`** 入队。
    sqlite_queue: Option<std::sync::Arc<FeishuImEventSqliteQueue>>,
    /// 与飞书 worker 单线程一致：避免并发 `POST /workspace` / CrabMate 请求交错。
    turn_lock: Mutex<()>,
    last_workspace_path: Mutex<Option<String>>,
    /// `approval_session_id` → 等待中的 **`POST /feishu/tool-decision`**（仅 **`WaitHttp`**）。
    pending_tool_decisions: DashMap<String, PendingToolDecision>,
    /// 单会话（`chat_id`）当前挂起的审批 **`approval_session_id`**（**`WaitMessage`** 与 **`WaitHttp`** 均写入，便于 `@` 指令完成）。
    pending_tool_session_by_chat: DashMap<String, String>,
}

struct TenantTokenCache {
    token: String,
    /// 绝对 Unix 秒，预留 120s 刷新余量。
    expires_at: i64,
}

impl FeishuBridgeState {
    pub fn try_new(cfg: FeishuBridgeConfig) -> Result<Arc<Self>, FeishuBridgeInitError> {
        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .timeout(Duration::from_secs(120))
            .build()?;

        let sqlite_path = cfg
            .event_queue_sqlite_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from);

        let (event_tx, event_rx, sqlite_queue) = if cfg.async_worker {
            if let Some(ref path) = sqlite_path {
                let q = FeishuImEventSqliteQueue::new(
                    path.as_path(),
                    cfg.sqlite_queue_max_retries,
                    cfg.sqlite_queue_poll_ms,
                )?;
                (None, None, Some(std::sync::Arc::new(q)))
            } else if cfg.event_queue_capacity > 0 {
                let cap = cfg.event_queue_capacity.max(1);
                let (tx, rx) = mpsc::channel(cap);
                (Some(tx), Some(rx), None)
            } else {
                (None, None, None)
            }
        } else {
            (None, None, None)
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
            sqlite_queue,
            turn_lock: Mutex::new(()),
            last_workspace_path: Mutex::new(None),
            pending_tool_decisions: DashMap::new(),
            pending_tool_session_by_chat: DashMap::new(),
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

        if let Some(q) = state.sqlite_queue.clone() {
            let st = Arc::clone(&state);
            let lease = state.cfg.sqlite_queue_lease_secs.max(30);
            tokio::spawn(async move {
                run_sqlite_im_queue_consumer(st, q, lease).await;
            });
        }

        Ok(state)
    }
}

async fn run_sqlite_im_queue_consumer(
    state: Arc<FeishuBridgeState>,
    queue: std::sync::Arc<FeishuImEventSqliteQueue>,
    lease_secs: i64,
) {
    let idle = Duration::from_millis(queue.poll_idle_ms());
    loop {
        let now = time_unix_secs();
        let q = std::sync::Arc::clone(&queue);
        let claimed = tokio::task::spawn_blocking(move || {
            let _ = q.reclaim_expired(now);
            q.claim_one(now, lease_secs)
        })
        .await;

        let claimed = match claimed {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                error!(?e, "feishu sqlite queue claim failed");
                sleep(idle).await;
                continue;
            }
            Err(e) => {
                error!(?e, "feishu sqlite queue spawn_blocking join failed");
                sleep(idle).await;
                continue;
            }
        };

        let Some((id, json)) = claimed else {
            sleep(idle).await;
            continue;
        };

        let envelope: Value = match serde_json::from_str(&json) {
            Ok(v) => v,
            Err(e) => {
                error!(
                    ?e,
                    queue_id = id,
                    "feishu sqlite queue envelope json corrupt"
                );
                let q = std::sync::Arc::clone(&queue);
                let _ = tokio::task::spawn_blocking(move || q.mark_done(id)).await;
                continue;
            }
        };

        let st = Arc::clone(&state);
        match handle_im_message_receive(&st, &envelope).await {
            Ok(()) => {
                let q = std::sync::Arc::clone(&queue);
                if let Err(e) = tokio::task::spawn_blocking(move || q.mark_done(id)).await {
                    error!(?e, queue_id = id, "feishu sqlite mark_done join failed");
                }
            }
            Err(e) => {
                error!(?e, queue_id = id, "feishu sqlite queue handler failed");
                let msg = e.to_string();
                let q = std::sync::Arc::clone(&queue);
                if let Err(join_e) =
                    tokio::task::spawn_blocking(move || q.mark_retry_or_fail(id, &msg)).await
                {
                    error!(
                        ?join_e,
                        queue_id = id,
                        "feishu sqlite mark_retry join failed"
                    );
                }
            }
        }
    }
}

pub fn build_router(state: Arc<FeishuBridgeState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/feishu/events", post(feishu_events))
        .route("/feishu/tool-decision", post(feishu_tool_decision))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

#[derive(Debug, Deserialize)]
struct ToolDecisionBody {
    approval_session_id: String,
    /// **`deny`** / **`allow_once`** / **`allow_always`**（与 CrabMate **`POST /chat/approval`** 一致）。
    decision: String,
}

async fn feishu_tool_decision(
    State(st): State<Arc<FeishuBridgeState>>,
    headers: HeaderMap,
    Json(body): Json<ToolDecisionBody>,
) -> impl IntoResponse {
    let Some(secret) = st
        .cfg
        .tool_decision_secret
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "tool decision endpoint disabled" })),
        )
            .into_response();
    };

    let bearer_ok = headers
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| {
            s.strip_prefix("Bearer ")
                .or_else(|| s.strip_prefix("bearer "))
        })
        .is_some_and(|t| constant_time_eq_trimmed(t, secret));
    let key_ok = headers
        .get("x-api-key")
        .or_else(|| headers.get("X-API-Key"))
        .and_then(|h| h.to_str().ok())
        .is_some_and(|t| constant_time_eq_trimmed(t, secret));
    if !bearer_ok && !key_ok {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid or missing credential" })),
        )
            .into_response();
    }

    let session_id = body.approval_session_id.trim().to_string();
    if session_id.is_empty() || session_id.len() > 128 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid approval_session_id" })),
        )
            .into_response();
    }

    let decision = normalize_tool_decision(&body.decision);
    let Some(decision) = decision else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "decision must be deny / allow_once / allow_always" })),
        )
            .into_response();
    };

    let Some((_, pending)) = st.pending_tool_decisions.remove(&session_id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no pending approval for this session" })),
        )
            .into_response();
    };

    if pending.reply_tx.send(decision.to_string()).is_err() {
        warn!(%session_id, "tool decision receiver dropped");
        return (
            StatusCode::GONE,
            Json(json!({ "error": "approval waiter gone" })),
        )
            .into_response();
    }

    (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
}

fn constant_time_eq_trimmed(a: &str, b: &str) -> bool {
    let a = a.trim();
    let b = b.trim();
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    ab.ct_eq(bb).into()
}

fn normalize_tool_decision(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "deny" => Some("deny"),
        "allow_once" => Some("allow_once"),
        "allow_always" => Some("allow_always"),
        _ => None,
    }
}

fn feishu_message_command_decision(text: &str) -> Option<&'static str> {
    let t = text.trim();
    if t == "!拒绝" || t.eq_ignore_ascii_case("!deny") {
        return Some("deny");
    }
    if t == "!允许一次" || t.eq_ignore_ascii_case("!allow_once") {
        return Some("allow_once");
    }
    if t == "!永久允许" || t.eq_ignore_ascii_case("!allow_always") {
        return Some("allow_always");
    }
    None
}

async fn handle_card_action_trigger(st: &Arc<FeishuBridgeState>, v: &Value) -> Response {
    let Some((session_id, raw_dec)) = feishu_tool_card::parse_card_tool_decision(v) else {
        return Json(feishu_tool_card::card_callback_error_toast_zh(
            "无法识别该按钮数据",
        ))
        .into_response();
    };
    let Some(decision) = normalize_tool_decision(raw_dec.as_str()) else {
        return Json(feishu_tool_card::card_callback_error_toast_zh(
            "无效的审批选项",
        ))
        .into_response();
    };
    match st.pending_tool_decisions.remove(&session_id) {
        Some((_, pending)) => {
            if pending.reply_tx.send(decision.to_string()).is_err() {
                return Json(feishu_tool_card::card_callback_error_toast_zh(
                    "审批通道已关闭",
                ))
                .into_response();
            }
            Json(feishu_tool_card::card_callback_ack_toast_zh("已提交审批")).into_response()
        }
        None => Json(feishu_tool_card::card_callback_error_toast_zh(
            "没有待处理的审批（可能已超时或已处理）",
        ))
        .into_response(),
    }
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

    if feishu_tool_card::is_card_action_trigger_payload(&v) {
        return handle_card_action_trigger(&st, &v).await;
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
            if let Some(q) = &st.sqlite_queue {
                let qc = std::sync::Arc::clone(q);
                let payload = v.clone();
                match tokio::task::spawn_blocking(move || qc.enqueue(&payload)).await {
                    Ok(Ok(())) => {
                        tracing::debug!("feishu im.message.receive_v1 persisted to sqlite queue");
                        (StatusCode::OK, Json(json!({}))).into_response()
                    }
                    Ok(Err(e)) => {
                        error!(?e, "feishu sqlite queue enqueue failed");
                        (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(json!({
                                "error": "persistent queue write failed; retry later",
                                "code": "FEISHU_EVENT_QUEUE_SQLITE_ERROR"
                            })),
                        )
                            .into_response()
                    }
                    Err(e) => {
                        error!(?e, "feishu sqlite queue enqueue join failed");
                        (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(json!({ "error": "queue worker overloaded" })),
                        )
                            .into_response()
                    }
                }
            } else if let Some(tx) = &st.event_tx {
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

