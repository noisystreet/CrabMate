//! 将本机 user-data（`llm_overrides.json`）合并进进程 [`AgentConfig`]（对齐 Web/桌面侧栏）。

use crabmate_config::{AgentConfig, ExposeSecret};
use secrecy::SecretString;

use super::{LlmEndpointOverride, load_llm_overrides, read_secret_web_api_bearer};

fn fill_nonempty_string(dst: &mut String, src: Option<&String>) {
    if let Some(s) = src.map(|x| x.trim()).filter(|x| !x.is_empty()) {
        *dst = s.to_string();
    }
}

fn fill_nonempty_opt_string(dst: &mut Option<String>, src: Option<&String>) {
    if let Some(s) = src.map(|x| x.trim()).filter(|x| !x.is_empty()) {
        *dst = Some(s.to_string());
    }
}

fn apply_temperature(cfg: &mut AgentConfig, raw: Option<&String>) {
    let Some(s) = raw.map(|x| x.trim()).filter(|x| !x.is_empty()) else {
        return;
    };
    if let Ok(t) = s.parse::<f32>() {
        cfg.llm_sampling.temperature = t;
    }
}

fn apply_context_tokens(cfg: &mut AgentConfig, raw: Option<&String>) {
    let Some(s) = raw.map(|x| x.trim()).filter(|x| !x.is_empty()) else {
        return;
    };
    if let Ok(n) = s.parse::<u32>() {
        cfg.llm_sampling.llm_context_tokens = n.min(10_000_000);
    }
}

fn apply_thinking_mode(cfg: &mut AgentConfig, raw: Option<&String>) {
    let Some(mode) = raw.map(|s| s.trim().to_ascii_lowercase()) else {
        return;
    };
    match mode.as_str() {
        "on" | "enabled" | "true" | "1" => {
            cfg.llm_vendor_flags.llm_bigmodel_thinking = true;
            cfg.llm_vendor_flags.llm_kimi_thinking_disabled = false;
        }
        "off" | "disabled" | "false" | "0" => {
            cfg.llm_vendor_flags.llm_bigmodel_thinking = false;
            cfg.llm_vendor_flags.llm_kimi_thinking_disabled = true;
        }
        "server" | "" => {}
        _ => {}
    }
}

fn apply_client_endpoint(cfg: &mut AgentConfig, ep: &LlmEndpointOverride) {
    fill_nonempty_string(&mut cfg.llm.api_base, ep.api_base.as_ref());
    fill_nonempty_string(&mut cfg.llm.model, ep.model.as_ref());
    apply_temperature(cfg, ep.temperature.as_ref());
    apply_context_tokens(cfg, ep.llm_context_tokens.as_ref());
    apply_thinking_mode(cfg, ep.llm_thinking_mode.as_ref());
}

fn apply_executor_endpoint(cfg: &mut AgentConfig, ep: &LlmEndpointOverride) {
    fill_nonempty_opt_string(&mut cfg.llm.executor_model, ep.model.as_ref());
    // executor 无独立 api_base 字段；仅 model 覆盖与 Web executor_llm 对齐。
}

/// 用 **`$XDG_DATA_HOME/crabmate/llm_overrides.json`** 覆盖进程配置（非空字段才写入）。
///
/// 与 Web `merge_client_llm_body` 同源磁盘；**不含** `api_key`（密钥走系统钥匙串 / `API_KEY`）。
pub fn apply_user_data_llm_overrides(cfg: &mut AgentConfig) {
    let disk = load_llm_overrides();
    apply_client_endpoint(cfg, &disk.client_llm);
    apply_executor_endpoint(cfg, &disk.executor_llm);
}

/// 当 TOML / **`CM_WEB_API_BEARER_TOKEN`** 均为空时，从系统钥匙串填入 **`web_api_bearer_token`**
///（与 Web `/user-data/secrets/web-api-bearer`、`crabmate web-bearer set` 同源）。
///
/// 优先级：**环境变量 / TOML（已 finalize）** → **钥匙串**。已有非空配置时不覆盖。
pub fn apply_user_data_web_api_bearer(cfg: &mut AgentConfig) {
    if !ExposeSecret::expose_secret(&cfg.web_api.web_api_bearer_token)
        .trim()
        .is_empty()
    {
        return;
    }
    let Some(from_keyring) = read_secret_web_api_bearer() else {
        return;
    };
    let trimmed = from_keyring.trim();
    if trimmed.is_empty() {
        return;
    }
    cfg.web_api.web_api_bearer_token = SecretString::new(trimmed.to_string().into());
    tracing::info!(
        target: "crabmate",
        "web_api_bearer_token 未在 TOML/环境变量中设置：已从系统钥匙串加载（与 Web/桌面同源）"
    );
}
