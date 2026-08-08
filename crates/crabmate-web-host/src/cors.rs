//! 可选 CORS：仅在配置了非空 Origin 白名单时挂载 `tower_http::cors::CorsLayer`。

use std::time::Duration;

use axum::http::{HeaderName, HeaderValue, Method, header};
use tower_http::cors::{AllowOrigin, CorsLayer};

/// 跨 Origin 时须暴露给 JS 的响应头（与前端 `chat_stream` / 排障契约对齐）。
///
/// 未列入 `Access-Control-Expose-Headers` 时，浏览器会隐藏这些头，导致会话绑定与断线重连失效。
pub const CORS_EXPOSE_RESPONSE_HEADERS: &[&str] =
    &["x-conversation-id", "x-stream-job-id", "x-request-id"];

/// 将配置中的 Origin 字符串解析为合法 `HeaderValue`（trim；非法/空跳过）。
pub fn parse_cors_origin_header_values(allowed_origins: &[String]) -> Vec<HeaderValue> {
    allowed_origins
        .iter()
        .map(|o| o.trim())
        .filter(|o| !o.is_empty())
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect()
}

/// 空白名单 → `None`（不挂 CORS，默认同源）；非空 → 精确 Origin 列表（无 `*`）。
///
/// - Methods: GET / POST / PUT / DELETE / OPTIONS
/// - Allow headers: Authorization、Content-Type、Accept、`x-api-key`、`last-event-id`
/// - Expose headers: [`CORS_EXPOSE_RESPONSE_HEADERS`]
/// - `allow_credentials(false)`：凭证走 Bearer 头，不依赖 cookie
pub fn try_cors_layer(allowed_origins: &[String]) -> Option<CorsLayer> {
    let origins = parse_cors_origin_header_values(allowed_origins);
    if origins.is_empty() {
        return None;
    }
    let expose: Vec<HeaderName> = CORS_EXPOSE_RESPONSE_HEADERS
        .iter()
        .map(|n| HeaderName::from_static(n))
        .collect();
    Some(
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers([
                header::AUTHORIZATION,
                header::CONTENT_TYPE,
                header::ACCEPT,
                HeaderName::from_static("x-api-key"),
                HeaderName::from_static("last-event-id"),
            ])
            .expose_headers(expose)
            .max_age(Duration::from_secs(600))
            .allow_credentials(false),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_allowlist_yields_no_layer() {
        assert!(try_cors_layer(&[]).is_none());
        assert!(try_cors_layer(&[String::new(), "  ".into()]).is_none());
    }

    #[test]
    fn parses_and_trims_origins() {
        let vals = parse_cors_origin_header_values(&[
            " http://127.0.0.1:8081 ".into(),
            "".into(),
            "https://ui.example.com".into(),
            "   ".into(),
        ]);
        assert_eq!(vals.len(), 2);
        assert_eq!(vals[0].to_str().unwrap(), "http://127.0.0.1:8081");
        assert_eq!(vals[1].to_str().unwrap(), "https://ui.example.com");
        assert!(try_cors_layer(&["http://127.0.0.1:8081".into()]).is_some());
    }

    #[test]
    fn expose_list_covers_stream_session_headers() {
        assert!(CORS_EXPOSE_RESPONSE_HEADERS.contains(&"x-conversation-id"));
        assert!(CORS_EXPOSE_RESPONSE_HEADERS.contains(&"x-stream-job-id"));
        assert!(CORS_EXPOSE_RESPONSE_HEADERS.contains(&"x-request-id"));
        for name in CORS_EXPOSE_RESPONSE_HEADERS {
            assert!(
                HeaderName::from_static(name).as_str() == *name,
                "invalid expose header name: {name}"
            );
        }
    }
}
