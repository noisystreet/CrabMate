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
    let verify_sig = !matches!(
        env::var("FEISHU_VERIFY_SIGNATURE").as_deref(),
        Ok("0") | Ok("false") | Ok("no")
    );
    let listen: SocketAddr = env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:9988".into())
        .parse()?;

    let crabmate = std::sync::Arc::new(CrabmateClient::new(crabmate_base, crabmate_bearer)?);
    let cfg = FeishuBridgeConfig {
        app_id,
        app_secret,
        encrypt_key,
        verify_signature_when_possible: verify_sig,
        crabmate,
        dedup_ttl: Duration::from_secs(600),
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
