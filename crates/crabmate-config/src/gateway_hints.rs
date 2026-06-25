//! 按 **`model` / `api_base`** 推断网关族行为（与 [`crabmate_llm::vendor`] 同源规则；供 [`finalize`] 在无 `llm` crate 时默认值）。

use crate::types::AgentConfig;

/// **MiniMax** 常见 OpenAI 兼容 **`model`**：`MiniMax-…`（大小写不敏感）；兼容部分 **`abab`** 前缀旧 ID。
#[inline]
pub fn is_minimax_family_model_id(model: &str) -> bool {
    let b = model.as_bytes();
    const M: &[u8] = b"minimax-";
    if b.len() >= M.len() && b[..M.len()].eq_ignore_ascii_case(M) {
        return true;
    }
    const A: &[u8] = b"abab";
    b.len() >= A.len() && b[..A.len()].eq_ignore_ascii_case(A)
}

#[inline]
fn api_base_looks_minimax(base: &str) -> bool {
    base.to_ascii_lowercase().contains("minimax")
}

/// TOML/环境变量均未设置 **`llm_reasoning_split`** 时的默认值：**MiniMax** 网关为 **`true`**，否则 **`false`**。
#[inline]
pub fn default_llm_reasoning_split_for_gateway(model: &str, api_base: &str) -> bool {
    is_minimax_family_model_id(model) || api_base_looks_minimax(api_base)
}

/// 出站是否将独立 **`system`** 折叠进 **`user`**：**MiniMax** 为 **`true`**，其余为 **`false`**。
#[inline]
pub fn fold_system_into_user_for_config(cfg: &AgentConfig) -> bool {
    is_minimax_family_model_id(&cfg.llm.model) || api_base_looks_minimax(&cfg.llm.api_base)
}
