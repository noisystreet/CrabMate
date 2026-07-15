//! Web 设置侧 LLM 草稿与执行器草稿。

use leptos::prelude::*;

use crate::api::load_saved_model_presets_from_storage;
use crate::app::shell_prefs_storage;

#[derive(Clone, Copy)]
pub struct LLMSettingsSignals {
    pub llm_api_base_draft: RwSignal<String>,
    pub llm_api_base_preset_select: RwSignal<String>,
    pub llm_model_draft: RwSignal<String>,
    pub llm_temperature_draft: RwSignal<String>,
    pub llm_context_tokens_draft: RwSignal<String>,
    pub llm_thinking_mode_draft: RwSignal<String>,
    pub llm_api_key_draft: RwSignal<String>,
    pub llm_has_saved_key: RwSignal<bool>,
    pub llm_settings_feedback: RwSignal<Option<String>>,
    pub executor_llm_api_base_draft: RwSignal<String>,
    pub executor_llm_api_base_preset_select: RwSignal<String>,
    pub executor_llm_model_draft: RwSignal<String>,
    pub executor_llm_api_key_draft: RwSignal<String>,
    pub executor_llm_has_saved_key: RwSignal<bool>,
    pub executor_llm_settings_feedback: RwSignal<Option<String>>,
    pub client_llm_storage_tick: RwSignal<u64>,
    /// **`true`**：聊天请求不附带 **`readonly_tool_ttl_cache_secs`**，跟随服务端；**`false`**：附带 **`0`** 关闭只读 **`run_command`** 短时缓存。
    pub readonly_tool_ttl_cache_follow_server: RwSignal<bool>,
    pub selected_agent_role: RwSignal<Option<String>>,
    /// 用户已在底栏改选角色、尚未随下一条聊天提交；水合勿用服务端 `active_agent_role` 覆盖。
    pub agent_role_user_override: RwSignal<bool>,
    /// 本机已保存的多条模型预设（与扁平 `client_llm` 并存；用于设置页下拉选用）。
    pub saved_model_presets: RwSignal<Vec<crate::api::SavedModelPreset>>,
}

impl LLMSettingsSignals {
    pub fn new() -> Self {
        Self {
            llm_api_base_draft: RwSignal::new(String::new()),
            llm_api_base_preset_select: RwSignal::new(String::from("server")),
            llm_model_draft: RwSignal::new(String::new()),
            llm_temperature_draft: RwSignal::new(String::new()),
            llm_context_tokens_draft: RwSignal::new(String::new()),
            llm_thinking_mode_draft: RwSignal::new("server".to_string()),
            llm_api_key_draft: RwSignal::new(String::new()),
            llm_has_saved_key: RwSignal::new(false),
            llm_settings_feedback: RwSignal::new(None),
            executor_llm_api_base_draft: RwSignal::new(String::new()),
            executor_llm_api_base_preset_select: RwSignal::new(String::from("server")),
            executor_llm_model_draft: RwSignal::new(String::new()),
            executor_llm_api_key_draft: RwSignal::new(String::new()),
            executor_llm_has_saved_key: RwSignal::new(false),
            executor_llm_settings_feedback: RwSignal::new(None),
            client_llm_storage_tick: RwSignal::new(0),
            readonly_tool_ttl_cache_follow_server: RwSignal::new(
                crate::api::load_readonly_tool_ttl_cache_follow_server_from_storage(),
            ),
            selected_agent_role: RwSignal::new(shell_prefs_storage::read_agent_role_initial()),
            agent_role_user_override: RwSignal::new(false),
            saved_model_presets: RwSignal::new(load_saved_model_presets_from_storage()),
        }
    }
}

impl Default for LLMSettingsSignals {
    fn default() -> Self {
        Self::new()
    }
}
