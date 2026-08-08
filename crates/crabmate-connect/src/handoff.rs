//! URL 规范化与 Bearer hash 交接。

use url::Url;

/// 与前端 [`frontend/src/api/connect_handoff.rs`] 一致。
pub const BEARER_HASH_KEY: &str = "cm_web_api_bearer";

pub fn normalize_base_url(raw: &str) -> Result<Url, String> {
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
pub fn build_handoff_url(mut base: Url, bearer: &str) -> Url {
    let b = bearer.trim();
    if b.is_empty() {
        base.set_fragment(None);
    } else {
        let enc = urlencoding::encode(b);
        base.set_fragment(Some(&format!("{BEARER_HASH_KEY}={enc}")));
    }
    base
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
