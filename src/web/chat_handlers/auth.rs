//! Web API Bearer 中间件（`AGENT_WEB_API_BEARER_TOKEN`）。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::super::app_state::AppState;
use crate::config::ExposeSecret;
use crate::web::http_types::chat::ApiError;

fn is_valid_bearer_header(
    auth_header: Option<&axum::http::header::HeaderValue>,
    token: &str,
) -> bool {
    if token.is_empty() {
        return true;
    }
    let Some(raw) = auth_header else {
        return false;
    };
    let Ok(v) = raw.to_str() else {
        return false;
    };
    let expected = format!("Bearer {}", token);
    v.trim() == expected
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
    if is_valid_bearer_header(req.headers().get(header::AUTHORIZATION), token.as_str()) {
        return next.run(req).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(ApiError {
            code: "UNAUTHORIZED",
            message: "缺少或无效的 Authorization Bearer token".to_string(),
        }),
    )
        .into_response()
}
