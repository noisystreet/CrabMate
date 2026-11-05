//! 配置加载与 `API_KEY` 助手：各命令共用，**不缓存**配置。
//!
//! 可见性统一 `pub(super)`：只有 [`super`]（`cm_cli`）与其兄弟模块用。

use std::env;

use log::info;

use crate::config;

/// `--llm-context-tokens` 覆盖已加载配置的 `llm_context_tokens`（上限 10M，防手滑写天文数字）。
fn apply_cli_llm_context_tokens_override(
    mut cfg: config::AgentConfig,
    cli_tokens: Option<u32>,
) -> config::AgentConfig {
    if let Some(n) = cli_tokens {
        cfg.llm_sampling.llm_context_tokens = n.min(10_000_000);
    }
    cfg
}

/// `--config` 为空时回落到交互式 CLI 的 XDG 默认路径（与历史行为一致）。
pub(super) fn finalize_cli_config_path(explicit: Option<String>) -> Option<String> {
    if let Some(p) = explicit.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return Some(p.to_string());
    }
    config::resolve_interactive_cli_config_path(None).map(|p| p.to_string_lossy().into_owned())
}

/// 载入配置并叠加 user-data 覆盖（LLM / 工具 / Web Bearer 密钥槽）。
pub(super) fn load_cli_agent_config(
    config_path: Option<&str>,
    llm_context_tokens_cli: Option<u32>,
) -> Result<config::AgentConfig, Box<dyn std::error::Error>> {
    let mut cfg = apply_cli_llm_context_tokens_override(
        config::load_config_for_cli(config_path)?,
        llm_context_tokens_cli,
    );
    crate::user_data::apply_user_data_llm_overrides(&mut cfg);
    crate::user_data::apply_user_data_tool_overrides(&mut cfg);
    crate::user_data::apply_user_data_web_api_bearer(&mut cfg);
    Ok(cfg)
}

/// `crabmate models` / `crabmate probe`：`bearer` 时仍要求进程环境变量 **`API_KEY`** 非空。
pub(super) fn require_api_key_for_cli_models_probe(
    cfg: &config::AgentConfig,
) -> Result<String, std::io::Error> {
    let v = env::var("API_KEY").unwrap_or_default();
    if cfg.llm.llm_http_auth_mode == config::LlmHttpAuthMode::Bearer && v.trim().is_empty() {
        eprintln!(
            "请设置环境变量 API_KEY（当前 llm_http_auth_mode=bearer；models/probe 须从环境读取密钥）"
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "未设置环境变量 API_KEY",
        ));
    }
    Ok(v)
}

/// `serve` / `bench` / `e2e`：读取可选进程级 LLM Bearer（**仅环境变量 `API_KEY`**）。
///
/// 模型密钥权威在 Client 请求体 **`client_llm.api_key`**；服务端不再读 `client_llm` 钥匙串。
pub(super) fn read_llm_api_key_from_env_lenient(cfg: &config::AgentConfig) -> String {
    let from_env = env::var("API_KEY").unwrap_or_default();
    if !from_env.trim().is_empty() {
        return from_env;
    }
    if cfg.llm.llm_http_auth_mode == config::LlmHttpAuthMode::Bearer {
        info!(
            target: "crabmate",
            "API_KEY 未设置（llm_http_auth_mode=bearer）：对话请由 Client 发送 client_llm.api_key，或 export API_KEY 供 models/probe/进程回退"
        );
    }
    String::new()
}
