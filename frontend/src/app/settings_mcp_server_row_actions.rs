//! MCP 设置页：单服务器行操作按钮。

use leptos::prelude::*;

use super::settings_mcp_status::{McpSettingsSignals, spawn_probe_mcp_server};
use crate::i18n::{self, Locale};

#[component]
pub(crate) fn SettingsMcpServerRowActions(
    locale: RwSignal<Locale>,
    server_id: String,
    ctx: McpSettingsSignals,
) -> impl IntoView {
    let McpSettingsSignals { set_file, busy, .. } = ctx;
    let id_remove = server_id.clone();
    let id_probe = server_id;

    view! {
        <div class="settings-mcp-row-actions">
            <button
                type="button"
                class="settings-btn settings-btn-secondary"
                prop:disabled=move || busy.get()
                on:click=move |_| spawn_probe_mcp_server(ctx, id_probe.clone())
            >
                {move || i18n::settings_mcp_probe(locale.get())}
            </button>
            <button
                type="button"
                class="settings-btn settings-btn-danger"
                on:click=move |_| {
                    let sid = id_remove.clone();
                    set_file.update(|f| f.servers.retain(|s| s.id != sid));
                }
            >
                {move || i18n::settings_mcp_remove(locale.get())}
            </button>
        </div>
    }
}
