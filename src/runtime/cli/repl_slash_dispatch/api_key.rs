//! `/api-key` 斜杠命令与可复用的密钥状态行。

use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use crate::config::LlmHttpAuthMode;
use crate::config::SharedAgentConfig;
use crate::runtime::cli_repl_ui::CliReplStyle;

use super::super::repl_extras::ReplSlashHandled;
use super::shared::split_optional_no_persist;

/// `/api-key set` 与本进程内存上限对齐（与 HTTP/Web 侧提示一致）。
pub(super) const REPL_API_KEY_SLASH_MAX_CHARS: usize = 16384;

fn api_key_usage_lines_for_terminal() -> [&'static str; 3] {
    [
        "用法: /api-key status · /api-key set <密钥> [--no-persist] · /api-key clear [--no-persist]",
        "说明: 仅写入本进程内存（服务端已不再提供 client_llm 钥匙串槽；官方路径为 Client client_llm.api_key）。",
        "/config reload 不会清除本进程内存中的密钥；未设置环境变量 API_KEY 时可用此命令。",
    ]
}

pub(crate) async fn api_key_status_lines_owned(
    cfg_holder: &SharedAgentConfig,
    api_key_holder: &Arc<StdMutex<String>>,
) -> Vec<String> {
    let g = cfg_holder.read().await;
    let k = api_key_holder.lock().unwrap_or_else(|e| e.into_inner());
    let set = !k.trim().is_empty();
    drop(k);
    if g.llm.llm_http_auth_mode == LlmHttpAuthMode::None {
        vec![
            "当前 llm_http_auth_mode=none：发往 LLM 的请求不附带 Bearer，通常无需配置 API 密钥。"
                .to_string(),
        ]
    } else if set {
        vec!["[ok] 本进程已设置 LLM API 密钥（非空，值已隐藏）。".to_string()]
    } else {
        vec!["本进程尚未设置 LLM API 密钥（环境变量 API_KEY 与 /api-key 均为空）；发消息前请 /api-key set <密钥> 或 export API_KEY 后重启。".to_string()]
    }
}

pub(crate) fn api_key_clear_lines_owned_persist(
    api_key_holder: &Arc<StdMutex<String>>,
    persist: bool,
) -> Vec<String> {
    api_key_holder
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    let mut msg = "[ok] 已清除本进程内存中的 LLM API 密钥（环境变量 API_KEY 不受影响）".to_string();
    if persist {
        msg.push_str("（服务端已无 client_llm 钥匙串槽，忽略持久化）。");
    } else {
        msg.push_str("（--no-persist，仅内存）。");
    }
    vec![msg]
}

pub(crate) fn api_key_set_lines_owned(
    secret: String,
    api_key_holder: &Arc<StdMutex<String>>,
) -> Vec<String> {
    let (secret, persist) = split_optional_no_persist(&secret);
    api_key_set_lines_owned_persist(secret, api_key_holder, persist)
}

pub(crate) fn api_key_set_lines_owned_persist(
    secret: String,
    api_key_holder: &Arc<StdMutex<String>>,
    persist: bool,
) -> Vec<String> {
    if secret.len() > REPL_API_KEY_SLASH_MAX_CHARS {
        return vec![format!(
            "[err] 密钥过长（上限 {} 字符）。",
            REPL_API_KEY_SLASH_MAX_CHARS
        )];
    }
    *api_key_holder.lock().unwrap_or_else(|e| e.into_inner()) = secret;
    let mut msg = "[ok] 已写入本进程 LLM API 密钥（值已隐藏）".to_string();
    if persist {
        msg.push_str("（服务端已无 client_llm 钥匙串槽，忽略持久化）。");
    } else {
        msg.push_str("（--no-persist，仅内存）。");
    }
    vec![msg]
}

pub(super) fn slash_api_key_usage(style: &CliReplStyle) -> ReplSlashHandled {
    for line in api_key_usage_lines_for_terminal() {
        let _ = style.print_line(line);
    }
    ReplSlashHandled::Handled
}

pub(super) async fn slash_api_key_status(
    cfg_holder: &SharedAgentConfig,
    api_key_holder: &Arc<StdMutex<String>>,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let lines = api_key_status_lines_owned(cfg_holder, api_key_holder).await;
    for line in lines {
        if line.starts_with("[ok] ") {
            let _ = style.print_success(line.strip_prefix("[ok] ").unwrap_or(&line));
        } else {
            let _ = style.print_line(&line);
        }
    }
    ReplSlashHandled::Handled
}

pub(super) fn slash_api_key_clear_persist(
    api_key_holder: &Arc<StdMutex<String>>,
    style: &CliReplStyle,
    persist: bool,
) -> ReplSlashHandled {
    let lines = api_key_clear_lines_owned_persist(api_key_holder, persist);
    for line in lines {
        if line.starts_with("[err] ") {
            let _ = style.eprint_error(line.strip_prefix("[err] ").unwrap_or(&line));
        } else if line.starts_with("[ok] ") {
            let _ = style.print_success(line.strip_prefix("[ok] ").unwrap_or(&line));
        } else {
            let _ = style.print_line(&line);
        }
    }
    ReplSlashHandled::Handled
}

pub(super) fn slash_api_key_set(
    secret: String,
    api_key_holder: &Arc<StdMutex<String>>,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let lines = api_key_set_lines_owned(secret, api_key_holder);
    for line in lines {
        if line.starts_with("[err] ") {
            let _ = style.eprint_error(line.strip_prefix("[err] ").unwrap_or(&line));
        } else if line.starts_with("[ok] ") {
            let _ = style.print_success(line.strip_prefix("[ok] ").unwrap_or(&line));
        } else {
            let _ = style.print_line(&line);
        }
    }
    ReplSlashHandled::Handled
}
