//! HTTP 请求关联 id：入站 `x-request-id`（校验后沿用）或服务端生成；始终写回响应头。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;

/// 响应 / 入站关联头（小写；HTTP/2 规范化后仍可按此名查找）。
pub(crate) static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

const MAX_INBOUND_REQUEST_ID_LEN: usize = 128;

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

/// 最外层中间件：注入 [`RequestId`]，并在响应上设置 `x-request-id`。
pub(crate) async fn attach_request_id(mut req: Request, next: Next) -> Response {
    let id = resolve_request_id(req.headers().get(&X_REQUEST_ID));
    req.extensions_mut().insert(RequestId(id.clone()));
    let mut res = next.run(req).await;
    if let Ok(val) = HeaderValue::from_str(&id) {
        res.headers_mut().insert(X_REQUEST_ID.clone(), val);
    }
    res
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
}
