//! `crabmate-im-bridge`：飞书事件订阅 Webhook → CrabMate **`POST /chat/stream`** → 飞书回复消息。
//!
//! ## 环境变量
//!
//! | 变量 | 必填 | 说明 |
//! |------|------|------|
//! | `CM_BASE_URL` | 是 | CrabMate `serve` 根地址，如 `http://127.0.0.1:8080` |
//! | `CM_WEB_API_BEARER_TOKEN` | 是 | 与 CrabMate **`web_api_bearer_token`** / TOML **`web_api_bearer_token`** 相同 |
//! | `CM_WEB_API_BEARER` | 否 | 若未设置 **`CM_WEB_API_BEARER_TOKEN`** 则读此项（二选一即可） |
//! | `FEISHU_APP_ID` | 是 | 飞书应用 App ID |
//! | `FEISHU_APP_SECRET` | 是 | 飞书应用 App Secret |
//! | `FEISHU_ENCRYPT_KEY` | 否 | 事件订阅 Encrypt Key；若设置且请求带签名头则校验 |
//! | `FEISHU_VERIFY_SIGNATURE` | 否 | 默认 `1`；设为 `0` 关闭签名校验（不推荐） |
//! | `FEISHU_VERIFICATION_TOKEN` | 否 | 与控制台 **Verification Token** 一致时，校验事件 JSON 内 **`header.token`**（或顶层 **`token`**） |
//! | `FEISHU_REPLAY_MAX_SKEW_SECS` | 否 | 默认 **`600`**：仅在 **已完成签名校验** 的请求上校验 **`X-Lark-Request-Timestamp`** 与本地时间偏差；**`0`** 关闭 |
//! | `FEISHU_NONCE_DEDUP_SECS` | 否 | 默认 **`900`**：签名校验通过后对 **`X-Lark-Request-Nonce`** 去重；**`0`** 关闭 |
//! | `FEISHU_GROUP_REQUIRE_BOT_MENTION` | 否 | 默认 **`0`**；设为 **`1`** 时，群聊仅处理 **`mentions`** 中含本机器人（须配 **`FEISHU_BOT_OPEN_ID`**） |
//! | `FEISHU_BOT_OPEN_ID` | 否 | 机器人 **`open_id`**（与 `mentions` 中 `id.open_id` 对齐） |
//! | `FEISHU_MAX_MESSAGE_JSON_CHARS` | 否 | **`interactive`/未知类型** 等 `content` 摘要最大字符数，默认 **`12000`**（至少 **256**） |
//! | `FEISHU_ASYNC_WORKER` | 否 | 默认 **`1`**：`im.message.receive_v1` **先入队并立即 HTTP 200**，后台再调 CrabMate；**`0`** 为同步处理（适合调试） |
//! | `FEISHU_EVENT_QUEUE_SQLITE` | 否 | 若设置非空路径（且 **`FEISHU_ASYNC_WORKER=1`**）：**`im.message.receive_v1`** 写入 **SQLite 持久化队列**（进程重启不丢）；与内存队列 **`FEISHU_EVENT_QUEUE_CAPACITY`** 互斥（本路径优先） |
//! | `FEISHU_EVENT_QUEUE_CAPACITY` | 否 | **内存**异步队列长度，默认 **`100`**；满时返回 **503**（`FEISHU_EVENT_QUEUE_FULL`）；未配置 SQLite 时生效 |
//! | `FEISHU_SQLITE_QUEUE_MAX_RETRIES` | 否 | 默认 **`5`**（至少 **1**）：SQLite 队列项处理失败后的最大重试次数 |
//! | `FEISHU_SQLITE_QUEUE_POLL_MS` | 否 | 默认 **`200`**（至少 **50**）：SQLite worker 空闲轮询间隔（毫秒） |
//! | `FEISHU_SQLITE_QUEUE_LEASE_SECS` | 否 | 默认 **`600`**（至少 **30**）：SQLite 认领租约秒数，崩溃后过期可重新认领 |
//! | `FEISHU_WORKSPACE_ROOT_TEMPLATE` | 否 | 若设置：每通会话在调用 CrabMate 前 **`POST /workspace`**；可用 **`{chat_id}`** 占位（飞书 `message.chat_id`），须落在 CrabMate **`workspace_allowed_roots`** 内 |
//! | `FEISHU_TOOL_APPROVAL_MODE` | 否 | 默认 **`wait_message`**：`deny_all` \| **`default_allow_once`** \| **`wait_http`** \| **`wait_message`**（见设计文档） |
//! | `FEISHU_TOOL_DECISION_SECRET` | 条件 | **`wait_http`** 必填；保护 **`POST /feishu/tool-decision`**（`Authorization: Bearer …` 或 **`X-API-Key`**） |
//! | `FEISHU_TOOL_DECISION_TIMEOUT_SECS` | 否 | 默认 **`600`**（至少 **5**）：**`wait_http`** / **`wait_message`** 等待人工决策的最长秒数，超时按拒绝 |
//! | `FEISHU_QUIET_SSE_STATUS` | 否 | 默认 **`0`**；设为 **`1`** 时流式过程中**不**逐条推送 SSE 进度（省 QPS），仅开场提示 + 结束结果卡片 |
//! | `FEISHU_RESULT_CARD_MAX_CHARS` | 否 | 默认 **`3500`**（至少 **200**）：结束结果卡片内助手正文摘要最大字符数 |
//! | `FEISHU_IN_PLACE_PROGRESS_CARD` | 否 | 默认 **`0`**；设为 **`1`** 时开场发可 **PATCH** 的占位交互卡片，结束时用 **`PATCH /im/v1/messages/:message_id`** 原地更新为结果摘要（需卡片 **`update_multi: true`**；失败则回退为新卡片或文本） |
//! | `LISTEN_ADDR` | 否 | 默认 `127.0.0.1:9988` |
//! | `RUST_LOG` | 否 | 如 `info,crabmate_im_bridge=debug` |
//!
//! 飞书后台「事件订阅」→ **请求 URL** 填：`https://<你的域名>/feishu/events`（本地调试需内网穿透）。使用**工具审批交互卡片**时，请在同一应用内**订阅** **`card.action.trigger`**，并将 **卡片回调请求地址** 设为与上述相同的 URL（本服务在 **`POST /feishu/events`** 内一并处理 **`im.message.receive_v1`** 与 **`card.action.trigger`**）。
//!
//! **安全**：勿将真实密钥写入仓库；生产环境请使用 HTTPS 与网关。

use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crabmate_im_bridge::{
    CrabmateClient, FeishuBridgeConfig, FeishuBridgeState, FeishuToolApprovalMode, build_router,
};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cfg = feishu_bridge_config_from_env()?;
    let state = FeishuBridgeState::try_new(cfg)?;
    let listen: SocketAddr = env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:9988".into())
        .parse()?;
    let app = build_router(state).layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!(%listen, "crabmate-im-bridge listening");
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

fn feishu_bridge_config_from_env() -> Result<FeishuBridgeConfig, String> {
    let crabmate_base = env_req("CM_BASE_URL")?;
    let crabmate_bearer = env::var("CM_WEB_API_BEARER_TOKEN")
        .or_else(|_| env::var("CM_WEB_API_BEARER"))
        .map_err(|_| {
            "缺少 CM_WEB_API_BEARER_TOKEN（或与 serve 相同的 Bearer；可选别名 CM_WEB_API_BEARER）"
                .to_string()
        })?;
    let app_id = env_req("FEISHU_APP_ID")?;
    let app_secret = env_req("FEISHU_APP_SECRET")?;
    let encrypt_key = env::var("FEISHU_ENCRYPT_KEY")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let verification_token = env::var("FEISHU_VERIFICATION_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let verify_sig = !matches!(
        env::var("FEISHU_VERIFY_SIGNATURE").as_deref(),
        Ok("0") | Ok("false") | Ok("no")
    );
    let replay_max_skew = env_u64("FEISHU_REPLAY_MAX_SKEW_SECS", 600)? as i64;
    let nonce_dedup_secs = env_u64("FEISHU_NONCE_DEDUP_SECS", 900)?;
    let group_require_bot_mention = matches!(
        env::var("FEISHU_GROUP_REQUIRE_BOT_MENTION").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    );
    let bot_open_id = env::var("FEISHU_BOT_OPEN_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let tool_decision_secret = env::var("FEISHU_TOOL_DECISION_SECRET")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let tool_approval_mode = parse_tool_approval_mode(
        env::var("FEISHU_TOOL_APPROVAL_MODE")
            .unwrap_or_else(|_| "wait_message".into())
            .as_str(),
    )?;
    if tool_approval_mode == FeishuToolApprovalMode::WaitHttp && tool_decision_secret.is_none() {
        return Err(
            "FEISHU_TOOL_APPROVAL_MODE=wait_http requires FEISHU_TOOL_DECISION_SECRET".to_string(),
        );
    }
    let tool_decision_timeout_secs = env_u64("FEISHU_TOOL_DECISION_TIMEOUT_SECS", 600)?.max(5);
    let sqlite_queue_path = env::var("FEISHU_EVENT_QUEUE_SQLITE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let sqlite_max_retries = env_u64("FEISHU_SQLITE_QUEUE_MAX_RETRIES", 5)?.max(1) as u32;
    let sqlite_poll_ms = env_u64("FEISHU_SQLITE_QUEUE_POLL_MS", 200)?.max(50);
    let sqlite_lease_secs = env_u64("FEISHU_SQLITE_QUEUE_LEASE_SECS", 600)? as i64;

    let crabmate =
        Arc::new(CrabmateClient::new(crabmate_base, crabmate_bearer).map_err(|e| e.to_string())?);
    Ok(FeishuBridgeConfig {
        app_id,
        app_secret,
        encrypt_key,
        verify_signature_when_possible: verify_sig,
        verification_token,
        replay_timestamp_max_skew_secs: replay_max_skew,
        nonce_dedup_ttl: Duration::from_secs(nonce_dedup_secs),
        group_require_bot_mention,
        bot_open_id,
        crabmate,
        dedup_ttl: Duration::from_secs(600),
        max_message_content_json_chars: env_u64("FEISHU_MAX_MESSAGE_JSON_CHARS", 12000)?.max(256)
            as usize,
        async_worker: env_bool("FEISHU_ASYNC_WORKER", true)?,
        event_queue_capacity: env_u64("FEISHU_EVENT_QUEUE_CAPACITY", 100)?.max(1) as usize,
        workspace_root_template: env::var("FEISHU_WORKSPACE_ROOT_TEMPLATE")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        tool_approval_mode,
        tool_decision_secret,
        tool_decision_timeout_secs,
        quiet_sse_status: env_bool("FEISHU_QUIET_SSE_STATUS", false)?,
        result_card_max_body_chars: env_u64("FEISHU_RESULT_CARD_MAX_CHARS", 3500)?.max(200)
            as usize,
        in_place_progress_card: env_bool("FEISHU_IN_PLACE_PROGRESS_CARD", false)?,
        event_queue_sqlite_path: sqlite_queue_path,
        sqlite_queue_max_retries: sqlite_max_retries,
        sqlite_queue_poll_ms: sqlite_poll_ms,
        sqlite_queue_lease_secs: sqlite_lease_secs.max(30),
    })
}

fn env_req(name: &str) -> Result<String, String> {
    let s = env::var(name).map_err(|_| format!("missing environment variable {name}"))?;
    let t = s.trim().to_string();
    if t.is_empty() {
        return Err(format!("environment variable {name} is empty"));
    }
    Ok(t)
}

fn env_u64(name: &str, default: u64) -> Result<u64, String> {
    match env::var(name) {
        Err(_) => Ok(default),
        Ok(s) => {
            let t = s.trim();
            if t.is_empty() {
                Ok(default)
            } else {
                t.parse::<u64>()
                    .map_err(|_| format!("invalid unsigned integer for {name}: {s}"))
            }
        }
    }
}

fn env_bool(name: &str, default: bool) -> Result<bool, String> {
    match env::var(name) {
        Err(_) => Ok(default),
        Ok(s) => {
            let t = s.trim().to_ascii_lowercase();
            if t.is_empty() {
                return Ok(default);
            }
            match t.as_str() {
                "1" | "true" | "yes" | "on" => Ok(true),
                "0" | "false" | "no" | "off" => Ok(false),
                _ => Err(format!("invalid boolean for {name}: {s}")),
            }
        }
    }
}

fn parse_tool_approval_mode(raw: &str) -> Result<FeishuToolApprovalMode, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "wait_message" => Ok(FeishuToolApprovalMode::WaitMessage),
        "deny_all" | "deny" => Ok(FeishuToolApprovalMode::DenyAll),
        "default_allow_once" | "auto_allow_once" | "allow_once_auto" => {
            Ok(FeishuToolApprovalMode::DefaultAllowOnce)
        }
        "wait_http" | "http" => Ok(FeishuToolApprovalMode::WaitHttp),
        other => Err(format!(
            "invalid FEISHU_TOOL_APPROVAL_MODE: {other} (deny_all | default_allow_once | wait_http | wait_message)"
        )),
    }
}
