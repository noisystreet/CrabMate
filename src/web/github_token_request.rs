//! 从 HTTP 请求提取 GitHub user token（头优先于 Cookie），并挂入请求作用域。

use axum::extract::Request;
use axum::http::HeaderMap;
use axum::http::header::{COOKIE, HeaderName};
use axum::middleware::Next;
use axum::response::Response;

/// 壳客户端投递的 user access token。
pub(crate) static X_CRABMATE_GITHUB_TOKEN: HeaderName =
    HeaderName::from_static("x-crabmate-github-token");

/// Device Flow status：值为 `body` 时 JSON 一次性回传 `access_token`（壳用）。
pub(crate) static X_CRABMATE_GITHUB_TOKEN_DELIVERY: HeaderName =
    HeaderName::from_static("x-crabmate-github-token-delivery");

/// HttpOnly Cookie 名（浏览器 Device Flow 成功后种下）。
pub(crate) const GITHUB_TOKEN_COOKIE_NAME: &str = "crabmate_github_token";

/// 从请求头解析 token：`X-CrabMate-GitHub-Token` → Cookie `crabmate_github_token`。
pub(crate) fn extract_github_token_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(raw) = headers
        .get(&X_CRABMATE_GITHUB_TOKEN)
        .and_then(|v| v.to_str().ok())
    {
        let t = raw.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    cookie_value(headers, GITHUB_TOKEN_COOKIE_NAME)
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookie_hdr = headers.get(COOKIE)?.to_str().ok()?;
    for part in cookie_hdr.split(';') {
        let part = part.trim();
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        if k.trim() == name {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

pub(crate) fn wants_github_token_body_delivery(headers: &HeaderMap) -> bool {
    headers
        .get(&X_CRABMATE_GITHUB_TOKEN_DELIVERY)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("body"))
}

/// 将请求中的 GitHub token 放入 task-local，供本请求内 `git`/`gh`/clone 解析。
pub(crate) async fn attach_request_github_token(req: Request, next: Next) -> Response {
    let token = extract_github_token_from_headers(req.headers());
    crate::github_token::with_request_github_token(token, async move { next.run(req).await }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn prefers_header_over_cookie() {
        let mut h = HeaderMap::new();
        h.insert(
            X_CRABMATE_GITHUB_TOKEN.clone(),
            HeaderValue::from_static("from-header"),
        );
        h.insert(
            COOKIE,
            HeaderValue::from_static("crabmate_github_token=from-cookie"),
        );
        assert_eq!(
            extract_github_token_from_headers(&h).as_deref(),
            Some("from-header")
        );
    }

    #[test]
    fn reads_cookie_when_header_absent() {
        let mut h = HeaderMap::new();
        h.insert(
            COOKIE,
            HeaderValue::from_static("a=1; crabmate_github_token=ghu_abc; b=2"),
        );
        assert_eq!(
            extract_github_token_from_headers(&h).as_deref(),
            Some("ghu_abc")
        );
    }

    #[test]
    fn delivery_body_flag() {
        let mut h = HeaderMap::new();
        assert!(!wants_github_token_body_delivery(&h));
        h.insert(
            X_CRABMATE_GITHUB_TOKEN_DELIVERY.clone(),
            HeaderValue::from_static("body"),
        );
        assert!(wants_github_token_body_delivery(&h));
    }
}
