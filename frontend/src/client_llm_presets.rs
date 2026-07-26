//! Web「设置」中 `client_llm.api_base` 网关预设：与 CLI/TUI 共用 [`crabmate_types::llm_gateway_presets`]。

#[allow(unused_imports)] // 供设置页 / 测试按旧名引用预设表与类型。
pub use crabmate_types::{
    LLM_API_BASE_PRESETS as CLIENT_LLM_API_BASE_PRESETS,
    LlmApiBasePreset as ClientLlmApiBasePreset, api_base_select_value_for_draft,
};
