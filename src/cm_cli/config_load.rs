//! 配置加载与 `API_KEY` 助手：各命令共用，**不缓存**配置。
//!
//! 可见性统一 `pub(super)`：只有 [`super`]（`cm_cli`）与其兄弟模块用。

use std::env;

use log::info;

use crate::config;
use crate::config::layer_precedence::{ConfigLayer, LayerTrace, apply_post_load_layers};

/// `--llm-context-tokens` 覆盖已加载配置的 `llm_context_tokens`（上限 10M，防手滑写天文数字）。
fn apply_cli_llm_context_tokens_override(cfg: &mut config::AgentConfig, cli_tokens: Option<u32>) {
    if let Some(n) = cli_tokens {
        cfg.llm_sampling.llm_context_tokens = n.min(10_000_000);
    }
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
    load_cli_agent_config_traced(config_path, llm_context_tokens_cli).map(|(cfg, _)| cfg)
}

/// 一层在最终 `AgentConfig` 上的**实际写入**（生产口径）。
///
/// 抽成具名函数是为了让测试能直接钉住「这一层做了什么」：删掉任一
/// `apply_user_data_*`（或 CLI 覆盖）都会让本模块的用例变红——层表驱动之后，
/// 「顺序对不对」有 golden 守，「这一步还在不在」就靠这里。
///
/// `cli_tokens` 是 `&mut Option<_>`：CLI 层只消费一次，重复访问不会二次生效。
fn apply_layer(
    cfg: &mut config::AgentConfig,
    layer: ConfigLayer,
    cli_tokens: &mut Option<u32>,
) {
    match layer {
        ConfigLayer::UserData => {
            crate::user_data::apply_user_data_llm_overrides(cfg);
            crate::user_data::apply_user_data_tool_overrides(cfg);
            crate::user_data::apply_user_data_web_api_bearer(cfg);
        }
        ConfigLayer::Cli => apply_cli_llm_context_tokens_override(cfg, cli_tokens.take()),
        // 这三层在 `cm_config::load_config_traced` 的 builder 阶段按同一张表执行。
        ConfigLayer::Embedded | ConfigLayer::UserToml | ConfigLayer::Env => {}
    }
}

/// 与 [`load_cli_agent_config`] 相同，另返回层溯源（`crabmate doctor` 用，#1342）。
///
/// 层序由 `config::layer_precedence::LAYERS` **单一表述**驱动：这里只写「每层怎么改」，
/// 不再用调用先后表达优先级（#1340 的根治项）。
pub(super) fn load_cli_agent_config_traced(
    config_path: Option<&str>,
    llm_context_tokens_cli: Option<u32>,
) -> Result<(config::AgentConfig, LayerTrace), Box<dyn std::error::Error>> {
    let (mut cfg, mut trace) = config::load_config_traced(config_path).map_err(|e| {
        eprintln!("{e}");
        std::io::Error::new(std::io::ErrorKind::InvalidData, e)
    })?;
    let mut cli_tokens = llm_context_tokens_cli;
    apply_post_load_layers(&mut cfg, &mut trace, |layer, cfg| {
        apply_layer(cfg, layer, &mut cli_tokens);
    });
    warn_on_context_gate_misalignment(&cfg);
    Ok((cfg, trace))
}

/// 最终 cfg 上的两道闸自检文案；`None` 表示一致或 token 预算未启用。
///
/// 触发条件（`count_gate_warning` 内）：`max_message_history × 假定 token/条 < trigger_tokens`，
/// 此时条数闸会先动手，用户观感是「上下文水位才两三成就丢历史」（#1339）。
///
/// 独立成函数是为了让测试能在**不抓日志**的前提下断言「最终 cfg 是否该告警」——
/// 层序一旦被改坏（例如 CLI 覆盖挪到 user-data 之前），`load_cli_agent_config` 的
/// 自检输入就不再是最终值，这条断言会红。
fn context_gate_warning(cfg: &config::AgentConfig) -> Option<String> {
    config::context_gates::count_gate_warning(cfg)
}

/// 两道闸（条数 / token 预算）的等效水位不一致时告警（#1342）。
///
/// 必须在层序**都应用之后**调用（本模块的 `load_cli_agent_config`）：`config::load_config`
/// 那一层还没有 user-data 的窗口覆盖，看到的不是最终生效值。
fn warn_on_context_gate_misalignment(cfg: &config::AgentConfig) {
    if let Some(message) = context_gate_warning(cfg) {
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
mod tests;
