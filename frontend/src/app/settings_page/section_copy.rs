//! 设置页分区标题与说明文案（从 `settings_page` 拆出以降低 nloc 棘轮）。

use crate::i18n::{self, Locale};

use super::hash_routing::SettingsSection;

pub(super) fn section_title(section: SettingsSection, locale: Locale) -> &'static str {
    match section {
        SettingsSection::Appearance => i18n::settings_section_appearance_title(locale),
        SettingsSection::Llm => i18n::settings_section_llm_title(locale),
        SettingsSection::ExecutorLlm => i18n::settings_section_executor_llm_title(locale),
        SettingsSection::Tools => i18n::settings_section_tools_title(locale),
        SettingsSection::Mcp => i18n::settings_section_mcp_title(locale),
        SettingsSection::Session => i18n::settings_section_session_title(locale),
        SettingsSection::Shortcuts => i18n::settings_section_shortcuts_title(locale),
    }
}

pub(super) fn section_desc(section: SettingsSection, locale: Locale) -> &'static str {
    match section {
        SettingsSection::Appearance => i18n::settings_section_appearance_desc(locale),
        SettingsSection::Llm => i18n::settings_section_llm_desc(locale),
        SettingsSection::ExecutorLlm => i18n::settings_section_executor_llm_desc(locale),
        SettingsSection::Tools => i18n::settings_section_tools_desc(locale),
        SettingsSection::Mcp => i18n::settings_section_mcp_desc(locale),
        SettingsSection::Session => i18n::settings_section_session_desc(locale),
        SettingsSection::Shortcuts => i18n::settings_section_shortcuts_desc(locale),
    }
}
