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
    /// 回合来源。**目前三值**，均为 `&'static str` 的枚举式取值（便于日志/指标聚合）：
    ///
    /// - `http`：浏览器 / API 客户端（`POST /chat`、`/chat/stream`、CLI）；
    /// - `scheduled`：`[[scheduled_agent_task]]` 定时任务；
    /// - `auto_resume`：`#1455` C 步的自动续跑——它**没有** HTTP 请求，但继承了某次用户请求的
    ///   授权，因此与 `scheduled` 分开记（两者的合规含义不同）。
    ///
    /// 新增取值等于给下游多一个类目：改这里要同步日志/指标消费方。
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

    /// `#1455` C 步：自动续跑回合的占位审计——它**没有** HTTP 请求。
    ///
    /// 与 [`Self::scheduled_placeholder`] 分开是为了让审计能区分「定时任务」与
    /// 「用户请求的自动续跑」：两者的合规含义不同（后者继承了某次用户请求的授权）。
    /// 本回合是否由自动续跑发起（`#1455` C 步）。
    ///
    /// 用它而不是新增 envelope 字段：`source` 本来就是枚举式取值（`http`/`scheduled`/
    /// `auto_resume`），续跑入队时就设成 `auto_resume`，收尾处直接读得到。
    pub(crate) fn is_auto_resume(&self) -> bool {
        self.source == "auto_resume"
    }

    pub(crate) fn auto_resume_placeholder() -> Self {
        Self {
            peer_ip: IpAddr::from([0, 0, 0, 0]),
            client_ip: "auto_resume".to_string(),
            bearer_fp: None,
            source: "auto_resume",
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
