//! Web HTTP 请求上下文：写副作用工具审计（与共享 Bearer 配套；**不**记录密钥明文）。

use std::net::SocketAddr;

use axum::http::{HeaderMap, header};
use subtle::ConstantTimeEq;

use crate::config::{AgentConfig, ExposeSecret};
use crate::request_audit::web_api_bearer_fingerprint;
use crate::web::chat_handlers::WEB_API_X_API_KEY_HEADER;

pub(crate) use crate::request_audit::WebRequestAudit;

fn first_forwarded_client_ip(raw: &str) -> Option<String> {
    raw.split(',')
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 与常见反向代理 `X-Forwarded-For` 头名一致（小写，HTTP 头名不区分大小写）。
const X_FORWARDED_FOR: &str = "x-forwarded-for";

fn resolve_audit_client_ip(
    trust_x_forwarded_for: bool,
    headers: &HeaderMap,
    peer: SocketAddr,
) -> String {
    if trust_x_forwarded_for
        && let Some(v) = headers.get(X_FORWARDED_FOR).and_then(|h| h.to_str().ok())
        && let Some(ip) = first_forwarded_client_ip(v)
    {
        return ip;
    }
    peer.ip().to_string()
}

/// 从 HTTP 请求构建 [`WebRequestAudit`]（Bearer 指纹按**请求头**与配置密钥比对，二者一致则记录指纹）。
pub(crate) fn web_request_audit_from_http(
    cfg: &AgentConfig,
    headers: &HeaderMap,
    peer: SocketAddr,
) -> WebRequestAudit {
    let trust = cfg.web_api.web_audit_trust_x_forwarded_for;
    let client_ip = resolve_audit_client_ip(trust, headers, peer);
    let configured_fp = web_api_bearer_fingerprint(cfg);
    let bearer_fp = configured_fp.filter(|_| {
        secret_matches_web_api(
            cfg,
            headers.get(header::AUTHORIZATION),
            headers.get(WEB_API_X_API_KEY_HEADER),
        )
    });
    WebRequestAudit {
        peer_ip: peer.ip(),
        client_ip,
        bearer_fp,
        source: "http",
    }
}

fn secret_matches_web_api(
    cfg: &AgentConfig,
    auth: Option<&axum::http::HeaderValue>,
    x_api_key: Option<&axum::http::HeaderValue>,
) -> bool {
    let secret = cfg
        .web_api
        .web_api_bearer_token
        .expose_secret()
        .trim()
        .as_bytes();
    if secret.is_empty() {
        return false;
    }
    fn bearer_payload(h: &axum::http::HeaderValue) -> Option<Vec<u8>> {
        let raw = h.to_str().ok()?;
        let v = raw.trim();
        let p = "Bearer ";
        if v.len() < p.len() || !v.as_bytes()[..p.len()].eq_ignore_ascii_case(p.as_bytes()) {
            return None;
        }
        Some(v[p.len()..].trim().as_bytes().to_vec())
    }
    fn x_key_payload(h: &axum::http::HeaderValue) -> Option<Vec<u8>> {
        let t = h.to_str().ok()?.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.as_bytes().to_vec())
        }
    }
    if let Some(a) = auth.and_then(bearer_payload)
        && a.len() == secret.len()
        && bool::from(secret.ct_eq(&a))
    {
        return true;
    }
    if let Some(k) = x_api_key.and_then(x_key_payload)
        && k.len() == secret.len()
        && bool::from(secret.ct_eq(&k))
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use super::*;
    use axum::http::HeaderValue;
    use secrecy::SecretString;

    fn test_cfg(secret: &str) -> AgentConfig {
        let mut cfg = crate::config::load_config(None).expect("embed default config");
        cfg.web_api.web_api_bearer_token = SecretString::new(secret.to_string().into());
        cfg.web_api.web_audit_trust_x_forwarded_for = false;
        cfg
    }

    #[test]
    fn first_hop_parsing_trims() {
        assert_eq!(
            first_forwarded_client_ip(" 192.0.2.1 , 10.0.0.1 ").as_deref(),
            Some("192.0.2.1")
        );
    }

    #[test]
    fn audit_http_records_fp_when_bearer_matches() {
        let cfg = test_cfg("correct-token");
        let expected = web_api_bearer_fingerprint(&cfg).expect("fp");
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str("Bearer correct-token").expect("header value"),
        );
        let peer = SocketAddr::from_str("198.51.100.2:12345").expect("addr");
        let audit = web_request_audit_from_http(&cfg, &headers, peer);
        assert_eq!(audit.bearer_fp.as_deref(), Some(expected.as_str()));
        assert_eq!(audit.peer_ip.to_string(), "198.51.100.2");
        assert_eq!(audit.client_ip, "198.51.100.2");
        assert_eq!(audit.source, "http");
    }

    #[test]
    fn audit_http_records_fp_when_x_api_key_matches() {
        let cfg = test_cfg("x-key-val");
        let expected = web_api_bearer_fingerprint(&cfg).expect("fp");
        let mut headers = HeaderMap::new();
        headers.insert(
            WEB_API_X_API_KEY_HEADER,
            HeaderValue::from_str("x-key-val").expect("header value"),
        );
        let peer = SocketAddr::from_str("127.0.0.1:9").expect("addr");
        let audit = web_request_audit_from_http(&cfg, &headers, peer);
        assert_eq!(audit.bearer_fp.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn audit_http_no_fp_when_secret_mismatch() {
        let cfg = test_cfg("expected");
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str("Bearer wrong").expect("header value"),
        );
        let peer = SocketAddr::from_str("203.0.113.4:1").expect("addr");
        let audit = web_request_audit_from_http(&cfg, &headers, peer);
        assert!(audit.bearer_fp.is_none());
    }

    #[test]
    fn audit_client_ip_uses_x_forwarded_for_when_trusted() {
        let mut cfg = test_cfg("t");
        cfg.web_api.web_audit_trust_x_forwarded_for = true;
        let mut headers = HeaderMap::new();
        headers.insert(
            X_FORWARDED_FOR,
            HeaderValue::from_static("192.0.2.9, 10.0.0.1"),
        );
        let peer = SocketAddr::from_str("127.0.0.1:8080").expect("addr");
        let audit = web_request_audit_from_http(&cfg, &headers, peer);
        assert_eq!(audit.client_ip, "192.0.2.9");
        assert_eq!(audit.peer_ip.to_string(), "127.0.0.1");
    }

    #[test]
    fn audit_client_ip_ignores_x_forwarded_for_when_untrusted() {
        let mut cfg = test_cfg("t");
        cfg.web_api.web_audit_trust_x_forwarded_for = false;
        let mut headers = HeaderMap::new();
        headers.insert(X_FORWARDED_FOR, HeaderValue::from_static("192.0.2.9"));
        let peer = SocketAddr::from_str("198.18.0.1:443").expect("addr");
        let audit = web_request_audit_from_http(&cfg, &headers, peer);
        assert_eq!(audit.client_ip, "198.18.0.1");
    }
}
