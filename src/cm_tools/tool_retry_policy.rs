//! 工具失败透明重试策略的**纯逻辑**（无 IO，可单测）。PR4 在 `dispatch_tool` 外层接线。
//!
//! 资格门（§3.2，全部满足才进入重试循环）：
//! 1. `tool_retry_enabled == true`（默认关闭，opt-in）；
//! 2. `is_readonly_tool` 为真（排除 run_command / terminal_session / http_request / 写类 / MCP / 动态 / workflow）；
//! 3. 工具名不在 `tool_retry_denied_tools` 额外排除表；
//! 4. `http_fetch` 的 URL 须已匹配 `http_fetch_allowed_prefixes`（避免审批重提示）；
//! 5. `read_dir` 的 path 不得为外部路径（绝对路径或含 `..`，同样避免审批重提示）。
//!
//! 重试判定：`error_code ∈ tool_retry_error_codes` 且已尝试次数 < `max_attempts`；
//! 退避：第 n 次重试等待 `min(backoff_ms * 2^(n-1), 5000)`。

use std::collections::HashSet;
use std::sync::Arc;

use crate::cm_config::AgentConfig;

/// 退避上限（毫秒），与 `finalize.rs` 夹取后的 `tool_retry_backoff_ms` 上限无关（此为指数上限）。
const MAX_BACKOFF_MS: u64 = 5000;

/// 单次工具调用的透明重试预算（由 `[tool_registry]` 字段派生，`Arc` 共享避免每轮克隆）。
pub struct ToolRetrySpec {
    pub enabled: bool,
    pub max_attempts: u64,
    pub backoff_ms: u64,
    pub error_codes: Arc<HashSet<String>>,
    pub denied_tools: Arc<HashSet<String>>,
}

impl ToolRetrySpec {
    pub fn from_config(cfg: &AgentConfig) -> Self {
        let p = &cfg.tool_registry_policy;
        Self {
            enabled: p.tool_registry_tool_retry_enabled,
            max_attempts: p.tool_registry_tool_retry_max_attempts,
            backoff_ms: p.tool_registry_tool_retry_backoff_ms,
            error_codes: p.tool_registry_tool_retry_error_codes.clone(),
            denied_tools: p.tool_registry_tool_retry_denied_tools.clone(),
        }
    }

    /// 资格门：该工具本次调用是否进入重试循环（含审批静态预检）。
    pub fn tool_retry_eligible(&self, cfg: &AgentConfig, tool_name: &str, args: &str) -> bool {
        if !self.enabled {
            return false;
        }
        if !crate::cm_tools::registry_policy::is_readonly_tool(cfg, tool_name) {
            return false;
        }
        if self.denied_tools.contains(tool_name) {
            return false;
        }
        if tool_name == "http_fetch" && http_fetch_args_need_approval(cfg, args) {
            return false;
        }
        if tool_name == "read_dir" && read_dir_args_has_external_path(args) {
            return false;
        }
        true
    }

    /// 第 `attempt` 次（从 1 起）尝试失败后，是否继续重试。
    pub fn should_retry(&self, error_code: Option<&str>, attempt: u64) -> bool {
        attempt < self.max_attempts && error_code.is_some_and(|c| self.error_codes.contains(c))
    }

    /// 第 `retry_index` 次（从 1 起）重试前的退避毫秒；`min(backoff * 2^(n-1), 5000)`。
    pub fn backoff_for_retry(&self, retry_index: u64) -> u64 {
        if self.backoff_ms == 0 {
            return 0;
        }
        let exp = retry_index.saturating_sub(1) as u32;
        self.backoff_ms
            .saturating_mul(2_u64.saturating_pow(exp))
            .min(MAX_BACKOFF_MS)
    }
}

/// `http_fetch` URL 是否需走审批（未匹配 `http_fetch_allowed_prefixes`）；需审批则**不重试**，
/// 避免 AllowOnce 后重跑再次弹审批。参数非法按保守处理（不可重试）。
fn http_fetch_args_need_approval(cfg: &AgentConfig, args: &str) -> bool {
    let Ok((url, _, _)) = crate::cm_tools::tools::http_fetch::parse_http_fetch_args(args) else {
        return true;
    };
    !crate::cm_tools::tools::http_fetch::url_matches_allowed_prefixes(
        &url,
        &cfg.http_fetch.http_fetch_allowed_prefixes,
    )
}

/// `read_dir` 入参 path 是否为外部路径（绝对路径或含 `..`）；外部路径走审批，**不重试**。
/// 无 `path` 参数（默认工作区根）或参数非法时按非外部处理。
fn read_dir_args_has_external_path(args: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(args) else {
        return false;
    };
    let Some(path) = v.get("path").and_then(|p| p.as_str()).map(str::trim) else {
        return false;
    };
    path.starts_with('/') || path.contains("..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_retry_enabled() -> AgentConfig {
        let mut cfg = crate::cm_config::load_config(None).expect("embed default");
        cfg.tool_registry_policy.tool_registry_tool_retry_enabled = true;
        cfg
    }

    #[test]
    fn from_config_defaults_off() {
        let cfg = crate::cm_config::load_config(None).expect("embed default");
        let spec = ToolRetrySpec::from_config(&cfg);
        assert!(!spec.enabled);
        assert_eq!(spec.max_attempts, 2);
        assert_eq!(spec.backoff_ms, 250);
        assert!(spec.error_codes.contains("timeout"));
        assert!(spec.error_codes.contains("http_timeout"));
        assert!(spec.error_codes.contains("rate_limited"));
        assert!(spec.error_codes.contains("http_network_error"));
        assert!(spec.denied_tools.is_empty());
    }

    #[test]
    fn eligible_requires_enabled_and_readonly() {
        let cfg_off = crate::cm_config::load_config(None).expect("embed default");
        let spec_off = ToolRetrySpec::from_config(&cfg_off);
        assert!(!spec_off.tool_retry_eligible(&cfg_off, "get_current_time", "{}"));

        let cfg = cfg_with_retry_enabled();
        let spec = ToolRetrySpec::from_config(&cfg);
        assert!(spec.tool_retry_eligible(&cfg, "get_current_time", "{}"));
        // run_command 非只读 → 不重试
        assert!(!spec.tool_retry_eligible(&cfg, "run_command", r#"{"command":"ls"}"#));
        // http_request 非只读 → 不重试
        assert!(!spec.tool_retry_eligible(&cfg, "http_request", r#"{"url":"https://e.com"}"#));
        // MCP 代理工具 → 不重试
        assert!(!spec.tool_retry_eligible(&cfg, "mcp__fanalyzer__fanalyzer_fetch", "{}"));
    }

    #[test]
    fn eligible_http_fetch_requires_prefix_match() {
        let mut cfg = cfg_with_retry_enabled();
        // 默认嵌入 `*` → 任意 http/https 直接执行 → 可重试
        let spec = ToolRetrySpec::from_config(&cfg);
        assert!(spec.tool_retry_eligible(
            &cfg,
            "http_fetch",
            r#"{"url":"https://example.com/a"}"#
        ));
        // 收紧前缀后未匹配 → 需审批 → 不重试
        cfg.http_fetch.http_fetch_allowed_prefixes =
            vec!["https://doc.rust-lang.org/".to_string()];
        let spec = ToolRetrySpec::from_config(&cfg);
        assert!(!spec.tool_retry_eligible(
            &cfg,
            "http_fetch",
            r#"{"url":"https://example.com/a"}"#
        ));
        assert!(spec.tool_retry_eligible(
            &cfg,
            "http_fetch",
            r#"{"url":"https://doc.rust-lang.org/book/"}"#
        ));
        // 参数非法 → 保守不可重试
        assert!(!spec.tool_retry_eligible(&cfg, "http_fetch", "not-json"));
    }

    #[test]
    fn eligible_read_dir_excludes_external_path() {
        let cfg = cfg_with_retry_enabled();
        let spec = ToolRetrySpec::from_config(&cfg);
        assert!(spec.tool_retry_eligible(&cfg, "read_dir", r#"{"path":"."}"#));
        assert!(spec.tool_retry_eligible(&cfg, "read_dir", r#"{"path":"src"}"#));
        // 无 path → 默认工作区根 → 可重试
        assert!(spec.tool_retry_eligible(&cfg, "read_dir", "{}"));
        assert!(!spec.tool_retry_eligible(&cfg, "read_dir", r#"{"path":"/etc"}"#));
        assert!(!spec.tool_retry_eligible(&cfg, "read_dir", r#"{"path":"../src"}"#));
    }

    #[test]
    fn eligible_respects_denied_tools() {
        let mut cfg = cfg_with_retry_enabled();
        cfg.tool_registry_policy.tool_registry_tool_retry_denied_tools =
            Arc::new(["get_current_time".to_string()].into_iter().collect());
        let spec = ToolRetrySpec::from_config(&cfg);
        assert!(!spec.tool_retry_eligible(&cfg, "get_current_time", "{}"));
    }

    #[test]
    fn should_retry_code_and_budget() {
        let cfg = cfg_with_retry_enabled();
        let spec = ToolRetrySpec::from_config(&cfg);
        assert!(spec.should_retry(Some("timeout"), 1));
        assert!(spec.should_retry(Some("http_timeout"), 1));
        // 超预算：attempts=2 时 attempt 1 → 可再试；attempt 2 → 停
        assert!(!spec.should_retry(Some("timeout"), 2));
        // 码未命中
        assert!(!spec.should_retry(Some("invalid_args"), 1));
        assert!(!spec.should_retry(Some("http_fetch_failed"), 1));
        assert!(!spec.should_retry(None, 1));
    }

    #[test]
    fn backoff_exponential_with_cap() {
        let cfg = cfg_with_retry_enabled();
        let spec = ToolRetrySpec::from_config(&cfg);
        assert_eq!(spec.backoff_for_retry(1), 250);
        assert_eq!(spec.backoff_for_retry(2), 500);
        assert_eq!(spec.backoff_for_retry(3), 1000);
        // 收敛上限 5000
        assert_eq!(spec.backoff_for_retry(6), 5000);
        assert_eq!(spec.backoff_for_retry(100), 5000);
        // backoff=0 → 0
        let spec0 = ToolRetrySpec {
            backoff_ms: 0,
            ..ToolRetrySpec::from_config(&cfg)
        };
        assert_eq!(spec0.backoff_for_retry(3), 0);
    }
}
