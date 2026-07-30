//! MCP 设置页：单条服务器编辑行。

use super::settings_mcp_server_row_actions::SettingsMcpServerRowActions;
use super::settings_mcp_status::McpSettingsSignals;
use super::settings_mcp_tools_list::SettingsMcpServerToolsList;
use super::settings_toggle_switch::SettingsToggleSwitch;
use crate::api::user_data::McpServersFileDto;
use crate::i18n;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

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

fn apply_has_bearer(file: &mut McpServersFileDto, server_id: &str, has_bearer: bool) {
    if let Some(row) = file.servers.iter_mut().find(|s| s.id == server_id) {
        row.has_bearer = has_bearer;
    }
}

#[component]
fn SettingsMcpRemoteBearer(
    server_id: String,
    locale: RwSignal<crate::i18n::Locale>,
    file: ReadSignal<McpServersFileDto>,
    set_file: WriteSignal<McpServersFileDto>,
    baseline: RwSignal<McpServersFileDto>,
    busy: ReadSignal<bool>,
    set_busy: WriteSignal<bool>,
) -> impl IntoView {
    let bearer_draft = RwSignal::new(String::new());
    let bearer_feedback = RwSignal::new(None::<String>);
    let id_hint = server_id.clone();
    let id_save = server_id;

    view! {
        <label class="settings-field">
            <span class="settings-field-label">
                {move || i18n::settings_mcp_bearer_label(locale.get())}
            </span>
            <input
                type="password"
                class="settings-input"
                autocomplete="off"
                data-testid="settings-mcp-bearer-input"
                prop:value=move || bearer_draft.get()
                placeholder="••••••••"
                on:input=move |ev| {
                    bearer_draft.set(event_input_value(&ev).unwrap_or_default());
                }
            />
        </label>
        <p class="settings-muted" data-testid="settings-mcp-bearer-hint">
            {move || {
                let sid = id_hint.clone();
                let set = file
                    .get()
                    .servers
                    .iter()
                    .find(|s| s.id == sid)
                    .is_some_and(|s| s.has_bearer);
                if set {
                    i18n::settings_mcp_bearer_hint_set(locale.get())
                } else {
                    i18n::settings_mcp_bearer_hint_unset(locale.get())
                }
            }}
        </p>
        <button
            type="button"
            class="btn btn-secondary btn-sm"
            data-testid="settings-mcp-bearer-save"
            prop:disabled=move || busy.get()
            on:click={
                let sid = id_save.clone();
                move |_| {
                    let loc = locale.get_untracked();
                    let token = bearer_draft.get_untracked();
                    let sid = sid.clone();
                    set_busy.set(true);
                    spawn_local(async move {
                        match crate::api::user_data::put_mcp_server_remote_auth(&sid, &token, loc)
                            .await
                        {
                            Ok(()) => {
                                let cleared = token.trim().is_empty();
                                let has_bearer = !cleared;
                                bearer_feedback.set(Some(if cleared {
                                    i18n::settings_mcp_bearer_cleared(loc).to_string()
                                } else {
                                    i18n::settings_mcp_bearer_saved(loc).to_string()
                                }));
                                bearer_draft.set(String::new());
                                set_file.update(|f| apply_has_bearer(f, &sid, has_bearer));
                                baseline.update(|f| apply_has_bearer(f, &sid, has_bearer));
                            }
                            Err(e) => bearer_feedback.set(Some(e)),
                        }
                        set_busy.set(false);
                    });
                }
            }
        >
            {move || i18n::settings_mcp_bearer_save(locale.get())}
        </button>
        <p class="settings-muted" data-testid="settings-mcp-bearer-feedback">
            {move || bearer_feedback.get().unwrap_or_default()}
        </p>
    }
}

#[component]
pub(crate) fn SettingsMcpServerRow(server_id: String, ctx: McpSettingsSignals) -> impl IntoView {
    let McpSettingsSignals {
        locale,
        file,
        set_file,
        baseline,
        status,
        probing,
        busy,
        set_busy,
        ..
    } = ctx;
    let id_row = server_id.clone();
    let id_hint = server_id.clone();
    let id_name_val = server_id.clone();
    let id_name_in = server_id.clone();
    let id_enabled_val = server_id.clone();
    let id_enabled_in = server_id.clone();
    let id_tools = server_id.clone();
    let id_remote = server_id.clone();
    let id_bearer = server_id;
    let tools_expanded = RwSignal::new(false);
    let show_bearer = Memo::new(move |_| {
        let sid = id_remote.clone();
        file.get()
            .servers
            .iter()
            .find(|s| s.id == sid)
            .is_some_and(|s| s.has_url)
    });

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
            <p class="settings-muted" data-testid="mcp-server-transport-hint">
                {move || {
                    let f = file.get();
                    let row = f.servers.iter().find(|s| s.id == id_hint);
                    match row {
                        Some(s) if s.has_url => i18n::settings_mcp_transport_remote(locale.get())
                            .to_string(),
                        Some(s) if s.has_command => i18n::settings_mcp_transport_stdio(locale.get())
                            .to_string(),
                        _ => String::new(),
                    }
                }}
            </p>
            <Show when=move || show_bearer.get()>
                <SettingsMcpRemoteBearer
                    server_id=id_bearer.clone()
                    locale=locale
                    file=file
                    set_file=set_file
                    baseline=baseline
                    busy=busy
                    set_busy=set_busy
                />
            </Show>
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
