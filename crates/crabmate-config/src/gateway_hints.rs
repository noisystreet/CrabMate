//! 按 **`model` / `api_base`** 推断网关族行为（与 [`crabmate_llm::vendor`] 同源规则；供 [`finalize`] 在无 `llm` crate 时默认值）。
//!
//! 代理到 `crabmate-types` 中的实现，避免 `crabmate-config` 重复定义。

/// TOML/环境变量均未设置 **`llm_reasoning_split`** 时的默认值：**MiniMax** 网关为 **`true`**，否则 **`false`**。
#[inline]
pub fn default_llm_reasoning_split_for_gateway(model: &str, api_base: &str) -> bool {
    crabmate_types::llm_config::default_llm_reasoning_split_for_gateway(model, api_base)
}

/// 出站是否将独立 **`system`** 折叠进 **`user`**：**MiniMax** 为 **`true`**，其余为 **`false`**。
#[inline]
pub fn fold_system_into_user_for_config(model: &str, api_base: &str) -> bool {
    crabmate_types::llm_config::fold_system_into_user_for_gateway(model, api_base)
}
