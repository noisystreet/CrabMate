//! 探测远程 `crabmate serve` 并导航主 WebView（Bearer 经 URL hash 交给前端消费）。

use std::time::Duration;

use tauri::{AppHandle, Manager, Url};

const BEARER_HASH_KEY: &str = "cm_web_api_bearer";
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectArgs {
    pub url: String,
    /// Web API 共享密钥（可空：服务端未启用 Bearer 时）。
    #[serde(default)]
    pub bearer: String,
}

fn normalize_base_url(raw: &str) -> Result<Url, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("请填写服务器地址".into());
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    let mut u = Url::parse(&with_scheme).map_err(|e| format!("地址无效: {e}"))?;
    match u.scheme() {
        "http" | "https" => {}
        other => return Err(format!("仅支持 http/https，收到 {other}")),
    }
    if !u.path().ends_with('/') {
        let p = if u.path().is_empty() {
            "/".to_string()
        } else {
            format!("{}/", u.path())
        };
        u.set_path(&p);
    }
    Ok(u)
}

/// 仅在 Bearer 非空时写入 hash，避免空交接清掉远程源已有凭证。
fn build_handoff_url(mut base: Url, bearer: &str) -> Url {
    let b = bearer.trim();
    if b.is_empty() {
        base.set_fragment(None);
    } else {
        let enc = urlencoding::encode(b);
        base.set_fragment(Some(&format!("{BEARER_HASH_KEY}={enc}")));
    }
    base
}

async fn probe_server(base: &Url, bearer: &str) -> Result<(), String> {
    let health = base
        .join("health")
        .map_err(|e| format!("无法构造 /health: {e}"))?;
    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))?;

    let mut req = client.get(health);
    let b = bearer.trim();
    if !b.is_empty() {
        req = req
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {b}"))
            .header("X-API-Key", b);
    }

    let resp = req.send().await.map_err(|e| {
        format!("无法连接服务器（网络/防火墙/地址错误，或明文 HTTP 被系统拦截）: {e}")
    })?;

    let status = resp.status();
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
        "服务器 /health 返回 HTTP {}，请确认 crabmate serve 已启动且地址正确",
        status.as_u16()
    ))
}

/// 探测 `/health` 后导航到远程 UI；非空 Bearer 经 URL hash 交接。
#[tauri::command]
pub async fn connect_remote(app: AppHandle, args: ConnectArgs) -> Result<(), String> {
    let base = normalize_base_url(&args.url)?;
    probe_server(&base, &args.bearer).await?;
    let target = build_handoff_url(base, &args.bearer);

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口未就绪".to_string())?;
    window
        .navigate(target)
        .map_err(|e| format!("无法打开远程界面: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_adds_scheme_and_slash() {
        let u = normalize_base_url("192.168.1.10:8080").unwrap();
        assert_eq!(u.scheme(), "http");
        assert!(u.path().ends_with('/'));
        assert!(u.as_str().contains("192.168.1.10:8080"));
    }

    #[test]
    fn handoff_puts_bearer_in_fragment_when_non_empty() {
        let base = normalize_base_url("http://127.0.0.1:8080").unwrap();
        let u = build_handoff_url(base, "a/b");
        assert_eq!(u.fragment().unwrap(), "cm_web_api_bearer=a%2Fb");
    }

    #[test]
    fn handoff_omits_fragment_when_bearer_empty() {
        let base = normalize_base_url("http://127.0.0.1:8080").unwrap();
        let u = build_handoff_url(base, "  ");
        assert!(u.fragment().is_none());
    }
}
