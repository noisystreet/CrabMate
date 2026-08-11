//! HTTP 请求关联 id：入站 `x-request-id`（校验后沿用）或服务端生成；始终写回响应头。
//!
//! 对小型 `application/json` 且形似 [`ApiError`](crabmate_api_contract::ApiError) 的响应体，
//! 若缺少 `request_id` 则补上与响应头同值的字段（handler 未手动挂载时亦一致）。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, to_bytes};
use axum::extract::Request;
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;

/// 响应 / 入站关联头（小写；HTTP/2 规范化后仍可按此名查找）。
pub(crate) static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

const MAX_INBOUND_REQUEST_ID_LEN: usize = 128;
/// 仅对不超过该大小的 JSON 错误体尝试注入 `request_id`（避免缓冲大响应）。
const MAX_JSON_INJECT_BYTES: usize = 64 * 1024;

/// 挂在请求 `Extensions` 与（有则）`ApiError.request_id` 上。
#[derive(Clone, Debug)]
pub(crate) struct RequestId(pub String);

static REQUEST_ID_SEQ: AtomicU64 = AtomicU64::new(1);

/// 生成服务端关联 id（无额外依赖；足够排障关联）。
pub(crate) fn generate_request_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let n = REQUEST_ID_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("cm-{millis:x}-{n:x}")
}

/// 入站头合法则沿用，否则生成新 id。
pub(crate) fn resolve_request_id(header_val: Option<&HeaderValue>) -> String {
    if let Some(raw) = header_val.and_then(|v| v.to_str().ok()) {
        let t = raw.trim();
        if is_valid_inbound_request_id(t) {
            return t.to_string();
        }
    }
    generate_request_id()
}

fn is_valid_inbound_request_id(s: &str) -> bool {
    if s.is_empty() || s.len() > MAX_INBOUND_REQUEST_ID_LEN {
        return false;
    }
    s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

/// 最外层中间件：注入 [`RequestId`]，写回 `x-request-id`，并尽量补齐 JSON `ApiError.request_id`。
pub(crate) async fn attach_request_id(mut req: Request, next: Next) -> Response {
    let id = resolve_request_id(req.headers().get(&X_REQUEST_ID));
    req.extensions_mut().insert(RequestId(id.clone()));
    let mut res = next.run(req).await;
    if let Ok(val) = HeaderValue::from_str(&id) {
        res.headers_mut().insert(X_REQUEST_ID.clone(), val);
    }
    inject_request_id_into_json_api_error(res, &id).await
}

fn content_type_is_json(res: &Response) -> bool {
    res.headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| {
            let base = ct.split(';').next().unwrap_or(ct).trim();
            base.eq_ignore_ascii_case("application/json")
        })
}

fn response_content_length(parts: &axum::http::response::Parts) -> Option<usize> {
    parts
        .headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

fn should_inject_request_id_into_body(res: &Response) -> bool {
    // 成功体不改写；SSE 等非 JSON 已在 content-type 处排除。
    (res.status().is_client_error() || res.status().is_server_error()) && content_type_is_json(res)
}

fn api_error_missing_request_id(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    let looks_like_api_error = obj.contains_key("code") && obj.contains_key("message");
    if !looks_like_api_error {
        return false;
    }
    match obj.get("request_id") {
        None | Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(s)) => s.is_empty(),
        _ => false,
    }
}

fn response_with_json_bytes(
    mut parts: axum::http::response::Parts,
    bytes: impl Into<axum::body::Bytes>,
) -> Response {
    let bytes = bytes.into();
    parts.headers.remove(CONTENT_LENGTH);
    if let Ok(len) = HeaderValue::from_str(&bytes.len().to_string()) {
        parts.headers.insert(CONTENT_LENGTH, len);
    }
    Response::from_parts(parts, Body::from(bytes))
}

/// 若体为带 `code`+`message` 的 JSON 对象且无 `request_id`，则写入与头同值的字段。
async fn inject_request_id_into_json_api_error(res: Response, id: &str) -> Response {
    if !should_inject_request_id_into_body(&res) {
        return res;
    }
    let (parts, body) = res.into_parts();
    if response_content_length(&parts).is_some_and(|n| n > MAX_JSON_INJECT_BYTES) {
        return Response::from_parts(parts, body);
    }
    let bytes = match to_bytes(body, MAX_JSON_INJECT_BYTES).await {
        Ok(b) => b,
        Err(_) => {
            // body 已消费且无法恢复；错误响应体过大或损坏时退回空 JSON 对象以免挂起连接
            return Response::from_parts(parts, Body::from("{}"));
        }
    };
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    let Some(obj) = value.as_object_mut() else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    if !api_error_missing_request_id(obj) {
        return Response::from_parts(parts, Body::from(bytes));
    }
    obj.insert(
        "request_id".to_string(),
        serde_json::Value::String(id.to_string()),
    );
    let Ok(new_bytes) = serde_json::to_vec(&value) else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    response_with_json_bytes(parts, new_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_overlong() {
        assert!(!is_valid_inbound_request_id(""));
        assert!(!is_valid_inbound_request_id(&"a".repeat(129)));
        assert!(is_valid_inbound_request_id("abc-123_XYZ.1:2"));
    }

    #[test]
    fn rejects_non_ascii_or_spaces() {
        assert!(!is_valid_inbound_request_id("bad id"));
        assert!(!is_valid_inbound_request_id("你好"));
    }

    #[test]
    fn resolve_keeps_valid_inbound() {
        let hv = HeaderValue::from_static("client-req-1");
        assert_eq!(resolve_request_id(Some(&hv)), "client-req-1");
    }

    #[test]
    fn resolve_generates_when_missing() {
        let a = resolve_request_id(None);
        assert!(a.starts_with("cm-"));
        let bad = HeaderValue::from_static("has space");
        let b = resolve_request_id(Some(&bad));
        assert!(b.starts_with("cm-"));
    }

    #[tokio::test]
    async fn injects_request_id_into_api_error_json() {
        let body = serde_json::json!({
            "code": "SSE_CLIENT_TOO_NEW",
            "message": "too new",
        });
        let res = Response::builder()
            .status(400)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let out = inject_request_id_into_json_api_error(res, "cm-test-1").await;
        let bytes = to_bytes(out.into_body(), MAX_JSON_INJECT_BYTES)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["request_id"], "cm-test-1");
        assert_eq!(v["code"], "SSE_CLIENT_TOO_NEW");
    }

    #[tokio::test]
    async fn does_not_overwrite_existing_request_id() {
        let body = serde_json::json!({
            "code": "UNAUTHORIZED",
            "message": "nope",
            "request_id": "keep-me",
        });
        let res = Response::builder()
            .status(401)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let out = inject_request_id_into_json_api_error(res, "cm-other").await;
        let bytes = to_bytes(out.into_body(), MAX_JSON_INJECT_BYTES)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["request_id"], "keep-me");
    }
}
