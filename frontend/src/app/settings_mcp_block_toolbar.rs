//! MCP 设置页：全局开关、超时与保存/探测操作栏。

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::settings_mcp_status::{
    McpSaveJob, McpSettingsSignals, spawn_probe_all_mcp, spawn_save_mcp,
};
use super::settings_toggle_switch::SettingsToggleSwitch;
use crate::api::user_data::McpServersFileDto;
use crate::i18n::{self, Locale};

fn event_input_value(ev: &leptos::ev::Event) -> Option<String> {
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|el| el.value())
}

#[component]
pub(crate) fn SettingsMcpBlockToolbar(
    locale: RwSignal<Locale>,
    file: ReadSignal<McpServersFileDto>,
    set_file: WriteSignal<McpServersFileDto>,
    import_json: RwSignal<String>,
    busy: ReadSignal<bool>,
    feedback: ReadSignal<Option<String>>,
    set_feedback: WriteSignal<Option<String>>,
    row_ctx: McpSettingsSignals,
) -> impl IntoView {
    view! {
        <SettingsToggleSwitch
            test_id="settings-mcp-global-enabled"
            checked=Signal::derive(move || file.get().global_enabled)
            label=Signal::derive(move || {
                i18n::settings_mcp_global_enabled_label(locale.get()).to_string()
            })
            on_toggle=move || set_file.update(|f| f.global_enabled = !f.global_enabled)
        />
        <label class="settings-field">
            <span class="settings-field-label">{move || i18n::settings_mcp_timeout_label(locale.get())}</span>
            <input
                type="number"
                min="1"
                class="settings-input"
                data-testid="settings-mcp-timeout"
                prop:value=move || file.get().tool_timeout_secs.to_string()
                on:input=move |ev| {
                    if let Some(v) = event_input_value(&ev)
                        && let Ok(n) = v.parse::<u64>()
                    {
                        set_file.update(|f| f.tool_timeout_secs = n.max(1));
                    }
                }
            />
        </label>
        <div class="settings-mcp-actions">
            <button
                type="button"
                class="settings-btn"
                data-testid="settings-mcp-save"
                prop:disabled=move || busy.get()
                on:click=move |_| {
                    set_feedback.set(None);
                    spawn_save_mcp(McpSaveJob {
                        loc: locale.get_untracked(),
                        pending_import: import_json.get_untracked(),
                        import_json,
                        ctx: row_ctx,
                        set_feedback,
                    });
                }
            >
                {move || i18n::settings_mcp_save(locale.get())}
            </button>
            <button
                type="button"
                class="settings-btn settings-btn-secondary"
                data-testid="settings-mcp-probe-all"
                prop:disabled=move || busy.get()
                on:click=move |_| spawn_probe_all_mcp(row_ctx)
            >
                {move || i18n::settings_mcp_probe_all(locale.get())}
            </button>
        </div>
        <Show when=move || feedback.get().is_some()>
            <p class="settings-intro settings-mcp-feedback">{move || feedback.get().unwrap_or_default()}</p>
        </Show>
    }
}
