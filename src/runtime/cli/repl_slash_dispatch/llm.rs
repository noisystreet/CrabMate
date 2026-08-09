//! `/model` 与 `/api-base` 斜杠命令实现。

use crate::config::SharedAgentConfig;
use crate::runtime::cli_repl_ui::CliReplStyle;

use super::super::repl_extras::{REPL_LLM_API_BASE_MAX, REPL_LLM_MODEL_MAX, ReplSlashHandled};
use super::shared::{persist_client_llm_overrides, split_optional_no_persist};

pub(super) async fn slash_model_show(
    cfg_holder: &SharedAgentConfig,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let cfg = cfg_holder.read().await;
    let _ = style.print_line(&format!("model: {}", cfg.llm.model));
    let _ = style.print_line(&format!("api_base: {}", cfg.llm.api_base));
    let _ = style.print_line(&format!(
        "temperature: {}（配置文件；Web chat 可单条覆盖）",
        cfg.llm_sampling.temperature
    ));
    if let Some(seed) = cfg.llm_sampling.llm_seed {
        let _ = style.print_line(&format!("llm_seed: {seed}"));
    } else {
        let _ = style.print_line("llm_seed: （未设置，请求不带 seed）");
    }
    let _ = style.print_line(
        "提示: /model set <名称> [--no-persist]；默认写入 user-data（与 Web 同源）；加 --no-persist 仅本进程。",
    );
    ReplSlashHandled::Handled
}

pub(super) async fn slash_model_set(
    name: String,
    cfg_holder: &SharedAgentConfig,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let (name, persist) = split_optional_no_persist(&name);
    let t = name.trim();
    if t.is_empty() {
        let _ = style.eprint_error(
            "用法: /model set <模型名或 id> [--no-persist]（可与 /models list 列出的 id 不同，不校验列表）",
        );
    } else if t.len() > REPL_LLM_MODEL_MAX {
        let _ = style.eprint_error(&format!("model 过长（上限 {REPL_LLM_MODEL_MAX} 字符）。"));
    } else {
        let label = t.to_string();
        let mut w = cfg_holder.write().await;
        w.llm.model.clone_from(&label);
        let model = w.llm.model.clone();
        let api_base = w.llm.api_base.clone();
        w.llm_vendor_flags.llm_reasoning_split =
            crabmate_types::llm_config::default_llm_reasoning_split_for_gateway(&model, &api_base);
        drop(w);
        let mut note = format!("已设 model = {label}");
        if persist {
            match persist_client_llm_overrides(Some(&label), None) {
                Ok(()) => note
                    .push_str("（已写入 user-data llm_overrides；/config reload 仍会再合并磁盘）"),
                Err(e) => {
                    let _ = style.eprint_error(&format!("写 user-data 失败: {e}"));
                    note.push_str("（仅本进程内存）");
                }
            }
        } else {
            note.push_str("（仅本进程；--no-persist）");
        }
        note.push_str("；llm_reasoning_split 已按网关默认刷新");
        let _ = style.print_success(&note);
    }
    ReplSlashHandled::Handled
}

pub(super) fn slash_model_usage(style: &CliReplStyle) -> ReplSlashHandled {
    let _ = style.eprint_error(
        "用法: /model（显示当前）· /model set <模型名或 id> [--no-persist]（默认写 user-data）",
    );
    ReplSlashHandled::Handled
}

pub(super) async fn slash_api_base_show(
    cfg_holder: &SharedAgentConfig,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let cfg = cfg_holder.read().await;
    let _ = style.print_line(&format!("api_base: {}", cfg.llm.api_base));
    let _ = style
        .print_line("提示: /api-base set <url|预设id> [--no-persist]（别名 /apibase）。预设 id：");
    for p in crabmate_types::llm_api_base_presets_with_url() {
        let model = p.suggested_model.unwrap_or("-");
        let _ = style.print_line(&format!("  {} → {}（建议 model: {model}）", p.id, p.url));
    }
    ReplSlashHandled::Handled
}

enum ApiBaseSetLabelError {
    TooLong,
    InvalidChars,
}

fn validate_api_base_set_label(label: &str) -> Result<(), ApiBaseSetLabelError> {
    if label.len() > REPL_LLM_API_BASE_MAX {
        Err(ApiBaseSetLabelError::TooLong)
    } else if label.contains('\0') || label.contains('\r') || label.contains('\n') {
        Err(ApiBaseSetLabelError::InvalidChars)
    } else {
        Ok(())
    }
}

async fn apply_api_base_set_label(
    label: String,
    suggested_model: Option<&str>,
    persist: bool,
    cfg_holder: &SharedAgentConfig,
    style: &CliReplStyle,
) {
    let mut w = cfg_holder.write().await;
    w.llm.api_base.clone_from(&label);
    if let Some(m) = suggested_model
        && w.llm.model.trim().is_empty()
    {
        w.llm.model = m.to_string();
    }
    let model = w.llm.model.clone();
    let api_base = w.llm.api_base.clone();
    w.llm_vendor_flags.llm_reasoning_split =
        crabmate_types::llm_config::default_llm_reasoning_split_for_gateway(&model, &api_base);
    drop(w);
    let mut note = format!("已设 api_base = {label}");
    if persist {
        match persist_client_llm_overrides(None, Some(&label)) {
            Ok(()) => note.push_str("（已写入 user-data llm_overrides）"),
            Err(e) => {
                let _ = style.eprint_error(&format!("写 user-data 失败: {e}"));
                note.push_str("（仅本进程内存）");
            }
        }
    } else {
        note.push_str("（仅本进程；--no-persist）");
    }
    note.push_str("；llm_reasoning_split 已按网关默认刷新");
    let _ = style.print_success(&note);
}

pub(super) async fn slash_api_base_set(
    url: String,
    cfg_holder: &SharedAgentConfig,
    style: &CliReplStyle,
) -> ReplSlashHandled {
    let (url, persist) = split_optional_no_persist(&url);
    let t = url.trim();
    if t.is_empty() {
        let _ = style.eprint_error(
            "用法: /api-base set <url|预设id> [--no-persist]（例如 https://api.openai.com/v1 或 deepseek）",
        );
    } else if let Some((label, suggested_model)) = crabmate_types::resolve_api_base_set_arg(t) {
        match validate_api_base_set_label(&label) {
            Err(ApiBaseSetLabelError::TooLong) => {
                let _ = style.eprint_error(&format!(
                    "api_base 过长（上限 {REPL_LLM_API_BASE_MAX} 字符）。"
                ));
            }
            Err(ApiBaseSetLabelError::InvalidChars) => {
                let _ = style.eprint_error("api_base 含非法控制字符，已拒绝。");
            }
            Ok(()) => {
                apply_api_base_set_label(label, suggested_model, persist, cfg_holder, style).await;
            }
        }
    } else {
        let _ = style.eprint_error(
            "无法解析 api_base：server/custom 无固定 URL，请直接写完整网关根地址，或使用 ollama/deepseek/minimax/zhipu/moonshot。",
        );
    }
    ReplSlashHandled::Handled
}

pub(super) fn slash_api_base_usage(style: &CliReplStyle) -> ReplSlashHandled {
    let _ = style.eprint_error(
        "用法: /api-base（显示当前与预设）· /api-base set <url|预设id> [--no-persist]（默认写 user-data）",
    );
    ReplSlashHandled::Handled
}
