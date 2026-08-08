//! 探测远程 `GET /health`，再探受保护的 `GET /user-data/prefs`（校验 Bearer）。

use std::time::Duration;

use url::Url;

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

fn attach_bearer(mut req: reqwest::RequestBuilder, bearer: &str) -> reqwest::RequestBuilder {
    let b = bearer.trim();
    if !b.is_empty() {
        req = req
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {b}"))
            .header("X-API-Key", b);
    }
    req
}

fn build_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(3))
        // 避免 http_proxy 把 127.0.0.1 / 局域网探测拐走。
        .no_proxy()
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))
}

async fn probe_get(
    client: &reqwest::Client,
    url: Url,
    bearer: &str,
    label: &str,
) -> Result<reqwest::Response, String> {
    let req = attach_bearer(client.get(url), bearer);
    req.send().await.map_err(|e| {
        format!("无法连接服务器（网络/防火墙/地址错误，或明文 HTTP 被系统拦截）[{label}]: {e}")
    })
}

fn map_auth_or_status(status: reqwest::StatusCode, label: &str) -> Result<(), String> {
    if status.is_success() {
        return Ok(());
    }
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(
            "服务器拒绝访问（401/403）。请填写与 CM_WEB_API_BEARER_TOKEN 一致的 Web API 共享密钥（不是模型 API_KEY）"
                .into(),
        );
    }
    Err(format!(
        "服务器 {label} 返回 HTTP {}，请确认 crabmate serve 已启动且地址正确",
        status.as_u16()
    ))
}

/// 先探 `/health`（可达性），再探 `/user-data/prefs`（鉴权；无密钥中间件时亦应 2xx）。
pub async fn probe_server(base: &Url, bearer: &str) -> Result<(), String> {
    let client = build_client()?;

    let health = base
        .join("health")
        .map_err(|e| format!("无法构造 /health: {e}"))?;
    let health_resp = probe_get(&client, health, bearer, "/health").await?;
    map_auth_or_status(health_resp.status(), "/health")?;

    let prefs = base
        .join("user-data/prefs")
        .map_err(|e| format!("无法构造 /user-data/prefs: {e}"))?;
    let prefs_resp = probe_get(&client, prefs, bearer, "/user-data/prefs").await?;
    map_auth_or_status(prefs_resp.status(), "/user-data/prefs")?;

    Ok(())
}
