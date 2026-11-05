//! 诊断类子命令：`doctor` / `config`（配置检查）/ `models` / `probe`。
//!
//! 四者都不初始化会话句柄：不建工具表、不建 `ProcessHandles`、不起后台任务。

use std::env;

use crate::cm_config::cli::CliGlobals;
use crate::config;
use crate::http_client;

/// `crabmate doctor`：一页本地诊断（人读、脱敏）。
pub(super) fn run_doctor(global: &CliGlobals) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = super::config_load::load_cli_agent_config(
        global.config_path.as_deref(),
        global.llm_context_tokens,
    )?;
    crate::runtime::cli_doctor::print_doctor_report(&cfg, global.workspace.as_deref());
    Ok(())
}

/// `crabmate config`：只做配置加载与密钥状态检查，随后退出。
pub(super) async fn run_dry_run(global: &CliGlobals) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = super::config_load::load_cli_agent_config(
        global.config_path.as_deref(),
        global.llm_context_tokens,
    )?;
    let key_note = match cfg.llm.llm_http_auth_mode {
        config::LlmHttpAuthMode::None => "llm_http_auth_mode=none（API_KEY 可选）".to_string(),
        config::LlmHttpAuthMode::Bearer => {
            let env_ok = env::var("API_KEY")
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            if env_ok {
                "llm_http_auth_mode=bearer 且 API_KEY 非空".to_string()
            } else {
                "llm_http_auth_mode=bearer：当前未设置 API_KEY（对话靠 Client client_llm.api_key；models/probe 仍须环境变量）"
                    .to_string()
            }
        }
    };
    println!("配置检查通过：{}（纯 API，不托管 UI）", key_note);
    Ok(())
}

/// `crabmate models` / `crabmate probe`：探测 `GET {api_base}/models`。
pub(super) async fn run_models_or_probe(
    global: &CliGlobals,
    probe: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = super::config_load::load_cli_agent_config(
        global.config_path.as_deref(),
        global.llm_context_tokens,
    )?;
    let api_key = super::config_load::require_api_key_for_cli_models_probe(&cfg)?;
    let client = http_client::build_shared_api_client(&cfg.llm_http_retry)?;
    if probe {
        crate::runtime::cli_doctor::run_probe_cli(&client, &cfg, api_key.trim()).await?;
    } else {
        crate::runtime::cli_doctor::run_models_cli(&client, &cfg, api_key.trim()).await?;
    }
    Ok(())
}
