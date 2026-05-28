//! MCP 设置页：单条服务器编辑行。

use super::settings_mcp_server_row_actions::SettingsMcpServerRowActions;
use super::settings_mcp_status::McpSettingsSignals;
use super::settings_mcp_tools_list::SettingsMcpServerToolsList;
use super::settings_toggle_switch::SettingsToggleSwitch;
use crate::api::user_data::McpServersFileDto;
use crate::i18n;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

fn event_input_value(ev: &leptos::ev::Event) -> Option<String> {
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|el| el.value())
}

fn server_field<F>(file: &McpServersFileDto, id: &str, pick: F) -> String
where
    F: Fn(&crate::api::user_data::McpServerEntryDto) -> String,
{
    file.servers
        .iter()
        .find(|s| s.id == id)
        .map(pick)
        .unwrap_or_default()
}

#[component]
pub(crate) fn SettingsMcpServerRow(server_id: String, ctx: McpSettingsSignals) -> impl IntoView {
    let McpSettingsSignals {
        locale,
        file,
        set_file,
        status,
        probing,
        ..
    } = ctx;
    let id_row = server_id.clone();
    let id_name_val = server_id.clone();
    let id_name_in = server_id.clone();
    let id_enabled_val = server_id.clone();
    let id_enabled_in = server_id.clone();
    let id_tools = server_id.clone();
    let tools_expanded = RwSignal::new(false);

    view! {
        <div
            class="settings-mcp-server-row"
            data-testid=format!("mcp-server-row-{}", id_row)
        >
            <label class="settings-field">
                <span class="settings-field-label">{move || i18n::settings_mcp_name_label(locale.get())}</span>
                <input
                    type="text"
                    class="settings-input"
                    prop:value=move || server_field(&file.get(), &id_name_val, |s| s.name.clone())
                    on:input=move |ev| {
                        let v = event_input_value(&ev).unwrap_or_default();
                        let sid = id_name_in.clone();
                        set_file.update(|f| {
                            if let Some(row) = f.servers.iter_mut().find(|s| s.id == sid) {
                                row.name = v;
                            }
                        });
                    }
                />
            </label>
            <SettingsMcpServerToolsList
                locale=locale
                server_id=id_tools.clone()
                status=status
                probing=probing
                expanded=tools_expanded
            />
            <SettingsToggleSwitch
                test_id="settings-mcp-server-enabled"
                checked=Signal::derive(move || {
                    file.get()
                        .servers
                        .iter()
                        .find(|s| s.id == id_enabled_val)
                        .is_some_and(|s| s.enabled)
                })
                label=Signal::derive(move || {
                    i18n::settings_mcp_enabled_label(locale.get()).to_string()
                })
                on_toggle={
                    let sid = id_enabled_in.clone();
                    move || {
                        set_file.update(|f| {
                            if let Some(row) = f.servers.iter_mut().find(|s| s.id == sid) {
                                row.enabled = !row.enabled;
                            }
                        });
                    }
                }
            />
            <SettingsMcpServerRowActions locale=locale server_id=id_tools ctx=ctx />
        </div>
    }
}
