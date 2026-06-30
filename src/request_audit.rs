//! HTTP 对话任务审计上下文（与 Axum 解耦的核心类型；HTTP 提取见 [`crate::web::audit`]）。

use std::net::IpAddr;

use sha2::{Digest, Sha256};

use crate::config::{AgentConfig, ExposeSecret};

/// 单次 HTTP 对话任务携带的审计上下文（队列 → `run_agent_turn`）。
#[derive(Debug, Clone)]
pub struct WebRequestAudit {
    /// 直连 TCP 对端（通常为反向代理地址）。
    pub peer_ip: IpAddr,
    /// 解析后的客户端 IP 字符串（优先 `X-Forwarded-For` 首跳，见配置）。
    pub client_ip: String,
    /// `Authorization` / `X-API-Key` 与配置 **`web_api_bearer_token`** 的 SHA-256 指纹（hex，前 12 字符）；密钥为空或未携带时为 `None`。
    pub bearer_fp: Option<String>,
    /// `http`（浏览器/API 客户端）或 `scheduled`（`[[scheduled_agent_task]]`）。
    pub source: &'static str,
}

impl WebRequestAudit {
    pub(crate) fn scheduled_placeholder() -> Self {
        Self {
            peer_ip: IpAddr::from([0, 0, 0, 0]),
            client_ip: "scheduled".to_string(),
            bearer_fp: None,
            source: "scheduled",
        }
    }
}

fn sha256_hex_prefix(secret: &[u8], prefix_hex_chars: usize) -> String {
    let digest = Sha256::digest(secret);
    let mut s = String::with_capacity(prefix_hex_chars.min(64));
    for b in digest.iter().take(prefix_hex_chars.div_ceil(2)) {
        use core::fmt::Write as _;
        let _ = write!(&mut s, "{b:02x}");
        if s.len() >= prefix_hex_chars {
            break;
        }
    }
    s.truncate(prefix_hex_chars.min(s.len()));
    s
}

/// 与配置中的共享 Web API 密钥对应的稳定指纹（不含明文）。
pub(crate) fn web_api_bearer_fingerprint(cfg: &AgentConfig) -> Option<String> {
    let raw = cfg.web_api.web_api_bearer_token.expose_secret();
    let b = raw.trim().as_bytes();
    if b.is_empty() {
        return None;
    }
    Some(sha256_hex_prefix(b, 12))
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::*;

    fn test_cfg(secret: &str) -> AgentConfig {
        let mut cfg = crate::config::load_config(None).expect("embed default config");
        cfg.web_api.web_api_bearer_token = SecretString::new(secret.to_string().into());
        cfg
    }

    #[test]
    fn web_api_bearer_fingerprint_stable_hex12() {
        let cfg = test_cfg("integration-test-secret");
        let fp = web_api_bearer_fingerprint(&cfg).expect("non-empty secret yields fingerprint");
        assert_eq!(fp.len(), 12);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(
            web_api_bearer_fingerprint(&cfg).as_deref(),
            Some(fp.as_str())
        );
    }
}
