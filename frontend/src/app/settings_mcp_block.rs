//! 设置页 MCP 多服务器配置块。

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use super::settings_mcp_block_toolbar::SettingsMcpBlockToolbar;
use super::settings_mcp_json_import::SettingsMcpJsonImportPanel;
use super::settings_mcp_server_row::SettingsMcpServerRow;
use super::settings_mcp_status::{McpSettingsSignals, refresh_mcp_status_with_probe};
use crate::api::user_data::{McpServerEntryDto, McpServersFileDto, McpServersStatusDto};
use crate::i18n::Locale;

fn spawn_reload_mcp(
    loc: Locale,
    set_file: WriteSignal<McpServersFileDto>,
    set_status: WriteSignal<Option<McpServersStatusDto>>,
    set_probing: WriteSignal<bool>,
) {
    spawn_local(async move {
        set_probing.set(true);
        if let Ok(f) = crate::api::user_data::fetch_mcp_servers(loc).await {
            set_file.set(f.clone());
            if let Some(st) = refresh_mcp_status_with_probe(loc, &f).await {
                set_status.set(Some(st));
            }
        }
        set_probing.set(false);
    });
}

#[component]
pub(crate) fn SettingsMcpBlock(locale: RwSignal<Locale>) -> impl IntoView {
    let (file, set_file) = signal(McpServersFileDto::default());
    let import_json = RwSignal::new(String::new());
    let (status, set_status) = signal(None::<McpServersStatusDto>);
    let (busy, set_busy) = signal(false);
    let (probing, set_probing) = signal(false);
    let (feedback, set_feedback) = signal(None::<String>);
    let row_ctx = McpSettingsSignals {
        locale,
        file,
        set_file,
        status,
        set_status,
        busy,
        set_busy,
        probing,
        set_probing,
    };

    Effect::new(move |_| {
        spawn_reload_mcp(locale.get(), set_file, set_status, set_probing);
    });

    view! {
        <div class="settings-block" data-testid="settings-mcp-block">
            <SettingsMcpJsonImportPanel
                locale=locale
                import_json=import_json
                set_file=set_file
                set_feedback=set_feedback
            />
            <SettingsMcpBlockToolbar
                locale=locale
                file=file
                set_file=set_file
                import_json=import_json
                busy=busy
                feedback=feedback
                set_feedback=set_feedback
                row_ctx=row_ctx
            />
            <For
                each=move || file.get().servers.clone()
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
