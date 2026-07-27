//! 终端侧上下文用量粗估（与 Web 底栏 tiktoken 同源计数）。

use crabmate_config::AgentConfig;
use crabmate_types::Message;

use crate::agent::tiktoken_prompt_tokens::prompt_token_count_vendor_shaped_for_session;

/// 一行摘要：`ctx ~used/cap` 或失败说明。
#[must_use]
pub(crate) fn context_usage_chip_line(cfg: &AgentConfig, messages: &[Message]) -> String {
    let cap = cfg.llm_sampling.llm_context_tokens.max(1);
    match prompt_token_count_vendor_shaped_for_session(cfg, messages) {
        Some(snap) => format!("ctx ~{}/{}", snap.prompt_tokens, cap),
        None => format!("ctx ?/{cap}"),
    }
}

/// 多行报告（REPL `/context`）。
#[must_use]
pub(crate) fn context_usage_report_lines(cfg: &AgentConfig, messages: &[Message]) -> Vec<String> {
    let cap = cfg.llm_sampling.llm_context_tokens.max(1);
    let char_budget = cfg.effective_context_char_budget_for_pipeline();
    let mut lines = vec![
        format!("llm_context_tokens（上限）: {cap}"),
        format!("effective_context_char_budget: {char_budget}"),
        format!("messages: {} 条", messages.len()),
    ];
    match prompt_token_count_vendor_shaped_for_session(cfg, messages) {
        Some(snap) => {
            let used = snap.prompt_tokens;
            let pct = (used as f64 / cap as f64 * 100.0).clamp(0.0, 999.0);
            lines.push(format!(
                "tiktoken prompt 粗估: ~{used}（约 {pct:.0}%；模型 {}；不含工具 JSON 细节，与网关计费可能有偏差）",
                snap.tiktoken_model
            ));
        }
        None => lines.push("tiktoken prompt 粗估: 不可用（模型无对应编码器或计数失败）".into()),
    }
    lines
}
