//! `crabmate-im-bridge`：飞书事件订阅 Webhook → CrabMate **`POST /chat`** → 飞书回复消息。
//!
//! ## 环境变量
//!
//! | 变量 | 必填 | 说明 |
//! |------|------|------|
//! | `CRABMATE_BASE_URL` | 是 | CrabMate `serve` 根地址，如 `http://127.0.0.1:8080` |
//! | `CRABMATE_WEB_API_BEARER` | 是 | 与 CrabMate **`web_api_bearer_token`** / **`CM_WEB_API_BEARER_TOKEN`** 相同 |
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
//! | `FEISHU_EVENT_QUEUE_CAPACITY` | 否 | 异步队列长度，默认 **`100`**；满时返回 **503**（`FEISHU_EVENT_QUEUE_FULL`）以便飞书重试 |
//! | `LISTEN_ADDR` | 否 | 默认 `127.0.0.1:9988` |
//! | `RUST_LOG` | 否 | 如 `info,crabmate_im_bridge=debug` |
//!
//! 飞书后台「事件订阅」请求 URL 填：`https://<你的域名>/feishu/events`（本地调试需内网穿透）。
//!
//! **安全**：勿将真实密钥写入仓库；生产环境请使用 HTTPS 与网关。

use std::env;
use std::net::SocketAddr;
use std::time::Duration;

use crabmate_im_bridge::{CrabmateClient, FeishuBridgeConfig, FeishuBridgeState, build_router};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let crabmate_base = env_req("CRABMATE_BASE_URL")?;
    let crabmate_bearer = env_req("CRABMATE_WEB_API_BEARER")?;
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
    let listen: SocketAddr = env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:9988".into())
        .parse()?;

    let crabmate = std::sync::Arc::new(CrabmateClient::new(crabmate_base, crabmate_bearer)?);
    let cfg = FeishuBridgeConfig {
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
    };
    let state = FeishuBridgeState::try_new(cfg)?;
    let app = build_router(state).layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!(%listen, "crabmate-im-bridge listening");
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
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
