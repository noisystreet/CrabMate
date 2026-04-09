//! Web API 鉴权中间件（`AGENT_WEB_API_BEARER_TOKEN` / `web_api_bearer_token`）。
//!
//! 受保护路径接受 **`Authorization: Bearer <token>`** 或 **`X-API-Key: <token>`**（与配置中的同一密钥比对）；二者满足其一即可。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use subtle::ConstantTimeEq;

use super::super::app_state::AppState;
use crate::config::ExposeSecret;
use crate::web::http_types::chat::ApiError;

/// 与常见 API 网关（Dify、Open WebUI 等）一致的备用请求头；值与 `web_api_bearer_token` 相同。
pub(crate) static WEB_API_X_API_KEY_HEADER: &str = "x-api-key";

fn secret_bytes_trimmed(token: &str) -> &[u8] {
    token.trim().as_bytes()
}

fn bearer_raw_value(auth_header: Option<&axum::http::header::HeaderValue>) -> Option<Vec<u8>> {
    let raw = auth_header?.to_str().ok()?;
    let v = raw.trim();
    let expected_prefix = "Bearer ";
    if v.len() < expected_prefix.len() {
        return None;
    }
    if !v.as_bytes()[..expected_prefix.len()].eq_ignore_ascii_case(expected_prefix.as_bytes()) {
        return None;
    }
    Some(v[expected_prefix.len()..].trim().as_bytes().to_vec())
}

fn x_api_key_raw_value(header_val: Option<&axum::http::header::HeaderValue>) -> Option<Vec<u8>> {
    let raw = header_val?.to_str().ok()?;
    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.as_bytes().to_vec())
    }
}

fn ct_eq_secret(secret: &[u8], candidate: &[u8]) -> bool {
    secret.len() == candidate.len() && bool::from(secret.ct_eq(candidate))
}

/// 若 `secret` 非空：`Authorization: Bearer <secret>` 或 `X-API-Key: <secret>` 任一匹配即通过。
fn is_authorized_for_web_api_secret(
    auth_header: Option<&axum::http::header::HeaderValue>,
    x_api_key_header: Option<&axum::http::header::HeaderValue>,
    secret: &str,
) -> bool {
    let secret = secret_bytes_trimmed(secret);
    if secret.is_empty() {
        return true;
    }
    if let Some(b) = bearer_raw_value(auth_header)
        && ct_eq_secret(secret, &b)
    {
        return true;
    }
    if let Some(k) = x_api_key_raw_value(x_api_key_header) {
        return ct_eq_secret(secret, &k);
    }
    false
}

pub(crate) async fn require_web_api_bearer_auth(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let token = {
        let g = state.cfg.read().await;
        g.web_api_bearer_token.expose_secret().trim().to_string()
    };
    if token.is_empty() {
        return next.run(req).await;
    }
    let auth = req.headers().get(header::AUTHORIZATION);
    let x_key = req.headers().get(WEB_API_X_API_KEY_HEADER);
    if is_authorized_for_web_api_secret(auth, x_key, token.as_str()) {
        return next.run(req).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(ApiError {
            code: "UNAUTHORIZED",
            message: "缺少或无效的 Web API 凭证（Authorization: Bearer 或 X-API-Key）".to_string(),
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_or_x_api_key_matches_same_secret() {
        let secret = "test-secret-xyz";
        let auth = axum::http::HeaderValue::from_str("Bearer test-secret-xyz").unwrap();
        assert!(is_authorized_for_web_api_secret(Some(&auth), None, secret));
        let key = axum::http::HeaderValue::from_str("test-secret-xyz").unwrap();
        assert!(is_authorized_for_web_api_secret(None, Some(&key), secret));
    }

    #[test]
    fn bearer_case_insensitive_prefix() {
        let secret = "abc";
        let auth = axum::http::HeaderValue::from_str("bearer abc").unwrap();
        assert!(is_authorized_for_web_api_secret(Some(&auth), None, secret));
    }

    #[test]
    fn wrong_bearer_but_valid_x_api_key_ok() {
        let secret = "good";
        let bad = axum::http::HeaderValue::from_str("Bearer bad").unwrap();
        let good = axum::http::HeaderValue::from_str("good").unwrap();
        assert!(is_authorized_for_web_api_secret(
            Some(&bad),
            Some(&good),
            secret
        ));
    }

    #[test]
    fn empty_secret_skips_auth() {
        assert!(is_authorized_for_web_api_secret(None, None, ""));
        assert!(is_authorized_for_web_api_secret(None, None, "   "));
    }

    #[test]
    fn mismatch_rejected() {
        assert!(!is_authorized_for_web_api_secret(None, None, "secret"));
        let wrong = axum::http::HeaderValue::from_str("Bearer other").unwrap();
        assert!(!is_authorized_for_web_api_secret(
            Some(&wrong),
            None,
            "secret"
        ));
    }
}
