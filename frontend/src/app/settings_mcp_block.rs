//! 设置页 MCP 多服务器配置块。

use leptos::prelude::*;

use super::settings_mcp_block_toolbar::SettingsMcpBlockToolbar;
use super::settings_mcp_json_import::SettingsMcpJsonImportPanel;
use super::settings_mcp_server_row::SettingsMcpServerRow;
use super::settings_mcp_status::McpSettingsPageState;
use crate::api::user_data::McpServerEntryDto;
use crate::i18n::Locale;

#[component]
pub(crate) fn SettingsMcpBlock(
    locale: RwSignal<Locale>,
    mcp: McpSettingsPageState,
) -> impl IntoView {
    let row_ctx = mcp.as_row_ctx(locale);

    view! {
        <div class="settings-block" data-testid="settings-mcp-block">
            <SettingsMcpJsonImportPanel
                locale=locale
                import_json=mcp.import_json
                set_file=mcp.set_file
                baseline=mcp.baseline
                set_feedback=mcp.set_feedback
            />
            <SettingsMcpBlockToolbar
                locale=locale
                file=mcp.file
                set_file=mcp.set_file
                import_json=mcp.import_json
                busy=mcp.busy
                feedback=mcp.feedback
                set_feedback=mcp.set_feedback
                row_ctx=row_ctx
            />
            <For
                each=move || mcp.file.get().servers.clone()
                key=|s| s.id.clone()
                children=move |srv: McpServerEntryDto| {
                    view! {
                        <SettingsMcpServerRow server_id=srv.id.clone() ctx=row_ctx />
                    }
                }
            />
            <p class="settings-intro settings-mcp-import-only-hint">
                {move || crate::i18n::settings_mcp_servers_via_json_hint(locale.get())}
            </p>
        </div>
    }
}
