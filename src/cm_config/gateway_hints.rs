//! 按 **`model` / `api_base`** 推断网关族行为（权威表 **`config/llm_vendors.toml`**，经 [`crate::cm_llm::vendor_catalog`]）。

/// TOML/环境变量均未设置 **`llm_reasoning_split`** 时的默认值（目录 **`default_reasoning_split`**）。
#[inline]
pub fn default_llm_reasoning_split_for_gateway(model: &str, api_base: &str) -> bool {
    crate::cm_llm::vendor_catalog::resolved_vendor_caps(model, api_base).default_reasoning_split
}

/// 出站是否将独立 **`system`** 折叠进 **`user`**（目录 **`fold_system_into_user`**）。
#[inline]
pub fn fold_system_into_user_for_config(model: &str, api_base: &str) -> bool {
    crate::cm_llm::vendor_catalog::resolved_vendor_caps(model, api_base).fold_system_into_user
}
