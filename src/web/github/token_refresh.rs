//! GitHub App refresh_token 代理刷新：`POST /github/oauth/token/refresh`。
//!
//! 壳在 body 提供 `refresh_token` / `client_id`；浏览器走 HttpOnly Cookie
//! `crabmate_github_refresh`（空 body 即可），成功后同时更新 token 与 refresh 两个 Cookie。
//! 共享常量与 Cookie / 表单辅助函数来自 [`super::device_flow`]。

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::web::app_state::AppStateHttpCore;
use crate::web::github_token_request::{
    GITHUB_REFRESH_COOKIE_NAME, GITHUB_TOKEN_COOKIE_NAME, extract_github_refresh_token_from_headers,
};

use super::device_flow::{
    GITHUB_ACCESS_TOKEN_URL, LAST_OAUTH_CLIENT_ID, err_json, form_body, parse_token_grant,
    set_token_cookie_header, validate_oauth_client_id,
};

#[derive(Debug, Deserialize, Default)]
pub(crate) struct TokenRefreshRequest {
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TokenRefreshResponse {
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token_expires_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// 显式 client_id 优先（须合法）；否则回退最近一次 Device Flow 记住的值。
fn resolve_refresh_client_id(explicit: Option<&str>, remembered: Option<&str>) -> Option<String> {
    if let Some(id) = explicit {
        return validate_oauth_client_id(id);
    }
    remembered
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 代理 GitHub refresh_token 刷新：壳在 body 提供 `refresh_token`（响应含轮换后的新对）；
/// 浏览器走 HttpOnly Cookie `crabmate_github_refresh`，成功后同时更新两个 Cookie。
pub(crate) async fn github_oauth_token_refresh_handler(
    State(http): State<AppStateHttpCore>,
    headers: HeaderMap,
    raw: Bytes,
) -> Response {
    let body: TokenRefreshRequest = if raw.is_empty() {
        TokenRefreshRequest::default()
    } else {
        serde_json::from_slice(&raw).unwrap_or_default()
    };

    let refresh_token = body
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| extract_github_refresh_token_from_headers(&headers));
    let Some(refresh_token) = refresh_token else {
        return err_json(
            StatusCode::BAD_REQUEST,
            "GITHUB_REFRESH_TOKEN_REQUIRED",
            "缺少 refresh_token（body 提供或浏览器 Cookie crabmate_github_refresh）",
        )
        .into_response();
    };

    let client_id = {
        let remembered = LAST_OAUTH_CLIENT_ID.lock().await;
        resolve_refresh_client_id(body.client_id.as_deref(), remembered.as_deref())
    };
    let Some(client_id) = client_id else {
        return err_json(
            StatusCode::BAD_REQUEST,
            "GITHUB_OAUTH_CLIENT_ID_REQUIRED",
            "缺少 client_id（请求体提供，或本进程已成功发起过 Device Flow）",
        )
        .into_response();
    };

    let form = form_body(&[
        ("client_id", client_id.as_str()),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token.as_str()),
    ]);
    let resp = match http
        .client
        .post(GITHUB_ACCESS_TOKEN_URL)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return err_json(
                StatusCode::BAD_GATEWAY,
                "GITHUB_TOKEN_REFRESH_FAILED",
                &format!("请求 GitHub access_token 失败：{e}"),
            )
            .into_response();
        }
    };
    let v: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return err_json(
                StatusCode::BAD_GATEWAY,
                "GITHUB_TOKEN_REFRESH_FAILED",
                &format!("解析 GitHub access_token 响应失败：{e}"),
            )
            .into_response();
        }
    };

    let Some(grant) = parse_token_grant(&v) else {
        let err = v.get("error").and_then(|x| x.as_str()).unwrap_or("");
        let msg = if err.is_empty() {
            "GitHub 未返回 access_token".to_string()
        } else {
            format!("GitHub 错误：{err}")
        };
        return err_json(StatusCode::UNAUTHORIZED, "GITHUB_REFRESH_TOKEN_REJECTED", &msg)
            .into_response();
    };

    // GitHub 轮换 refresh_token；未返回时沿用原值。
    let new_refresh = Some(grant.refresh_token.clone().unwrap_or(refresh_token));
    let mut res = Json(TokenRefreshResponse {
        access_token: grant.access_token.clone(),
        refresh_token: new_refresh.clone(),
        expires_in: grant.expires_in,
        refresh_token_expires_in: grant.refresh_token_expires_in,
        scope: grant.scope,
    })
    .into_response();
    if let Some(cookie) = set_token_cookie_header(GITHUB_TOKEN_COOKIE_NAME, &grant.access_token, &headers)
    {
        res.headers_mut().append(header::SET_COOKIE, cookie);
    }
    if let Some(r) = new_refresh.as_deref()
        && let Some(cookie) = set_token_cookie_header(GITHUB_REFRESH_COOKIE_NAME, r, &headers)
    {
        res.headers_mut().append(axum::http::header::SET_COOKIE, cookie);
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_client_id_prefers_explicit_valid() {
        assert_eq!(
            resolve_refresh_client_id(Some(" Iv1.abc123 "), Some("Iv1.old")).as_deref(),
            Some("Iv1.abc123")
        );
        assert!(resolve_refresh_client_id(Some("bad id"), Some("Iv1.old")).is_none());
        assert_eq!(
            resolve_refresh_client_id(None, Some("Iv1.old")).as_deref(),
            Some("Iv1.old")
        );
    }
}
