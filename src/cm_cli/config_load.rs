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
///
/// **顺序即契约**：user-data 覆盖先应用，**CLI 显式覆盖最后落定**。
/// 早期实现把 CLI 覆盖放在 user-data 之前，于是 `--llm-context-tokens` 会被
/// `~/.local/share/crabmate/llm_overrides.json` 的 `client_llm.llm_context_tokens` 静默盖掉（#1340）。
/// `--no-tools` 一类工具覆盖不受影响：`apply_user_data_tool_overrides` 只在既有禁用集上**追加**禁用，
/// 顺序互换不会让它把 CLI 的禁用放开。
pub(super) fn load_cli_agent_config(
    config_path: Option<&str>,
    llm_context_tokens_cli: Option<u32>,
) -> Result<config::AgentConfig, Box<dyn std::error::Error>> {
    let mut cfg = config::load_config_for_cli(config_path)?;
    crate::user_data::apply_user_data_llm_overrides(&mut cfg);
    crate::user_data::apply_user_data_tool_overrides(&mut cfg);
    crate::user_data::apply_user_data_web_api_bearer(&mut cfg);
    let cfg = apply_cli_llm_context_tokens_override(cfg, llm_context_tokens_cli);
    warn_on_context_gate_misalignment(&cfg);
    Ok(cfg)
}

/// 两道闸（条数 / token 预算）的等效水位不一致时告警（#1342）。
///
/// 触发条件：`max_message_history × 假定 token/条 < trigger_tokens` —— 此时条数闸会先动手，
/// 用户观感是「上下文水位才两三成就丢历史」（#1339）。
///
/// 必须在本函数（即 user-data 与 CLI 覆盖**都应用之后**）调用：`config::load_config` 那一层
/// 还没有 user-data 的窗口覆盖，看到的不是最终生效值。
fn warn_on_context_gate_misalignment(cfg: &config::AgentConfig) {
    if let Some(message) = config::context_gates::count_gate_warning(cfg) {
        log::warn!(target: "crabmate", "{message}");
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// #1340：`--llm-context-tokens` 必须在 user-data 覆盖**之后**落定，不能被它盖回。
    ///
    /// 测试直接模拟「user-data 已把窗口写成大值」这一步，再应用 CLI 覆盖：
    /// 顺序一旦被改回 user-data 在后，这条断言就会红。
    #[test]
    fn cli_llm_context_tokens_override_wins_over_user_data_layer() {
        let mut cfg = crate::cm_config::load_config(None).expect("embedded default config");
        cfg.llm_sampling.llm_context_tokens = 1_000_000;
        let cfg = apply_cli_llm_context_tokens_override(cfg, Some(100_000));
        assert_eq!(cfg.llm_sampling.llm_context_tokens, 100_000);
    }

    /// 未给 CLI 覆盖时，user-data 的值必须原样保留（不能顺手清成配置默认）。
    #[test]
    fn absent_cli_override_keeps_user_data_value() {
        let mut cfg = crate::cm_config::load_config(None).expect("embedded default config");
        cfg.llm_sampling.llm_context_tokens = 1_000_000;
        let cfg = apply_cli_llm_context_tokens_override(cfg, None);
        assert_eq!(cfg.llm_sampling.llm_context_tokens, 1_000_000);
    }

    /// `0` 视为「不覆盖」（见 `GlobalOpts::llm_context_tokens` 的说明），上限仍是 10M。
    #[test]
    fn cli_override_is_capped_at_ten_million() {
        let cfg = crate::cm_config::load_config(None).expect("embedded default config");
        let cfg = apply_cli_llm_context_tokens_override(cfg, Some(u32::MAX));
        assert_eq!(cfg.llm_sampling.llm_context_tokens, 10_000_000);
    }
}
