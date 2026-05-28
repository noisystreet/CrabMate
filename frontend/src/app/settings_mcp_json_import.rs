//! MCP JSON 粘贴导入（解析在服务端 `POST /user-data/mcp-servers/import`）。

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use gloo_timers::future::TimeoutFuture;

use crate::api::user_data::{McpServersFileDto, McpServersImportResponseDto};
use crate::i18n::{self, Locale};

/// 文本是否像 MCP 配置 JSON（用于粘贴后自动解析）。
pub fn looks_like_mcp_json(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    t.contains("mcpServers") || (t.starts_with('{') && t.contains("\"command\""))
}

pub fn format_import_feedback(
    loc: Locale,
    imported_count: usize,
    warnings: &[String],
    skipped_remote: &[String],
) -> String {
    let mut msg = i18n::settings_mcp_import_success(loc, imported_count);
    if !skipped_remote.is_empty() {
        msg.push('\n');
        msg.push_str(&i18n::settings_mcp_import_skipped_remote(
            loc,
            &skipped_remote.join(", "),
        ));
    }
    for w in warnings {
        msg.push('\n');
        msg.push_str(w);
    }
    msg
}

async fn post_import_json(text: &str, loc: Locale) -> Result<McpServersImportResponseDto, String> {
    crate::api::user_data::post_mcp_servers_import(text, loc).await
}

#[component]
pub(crate) fn SettingsMcpJsonImportPanel(
    locale: RwSignal<Locale>,
    import_json: RwSignal<String>,
    set_file: WriteSignal<McpServersFileDto>,
    set_feedback: WriteSignal<Option<String>>,
) -> impl IntoView {
    let apply_import = move |_| {
        spawn_local(async move {
            let loc = locale.get_untracked();
            let text = import_json.get_untracked();
            match post_import_json(&text, loc).await {
                Ok(outcome) => {
                    let msg = format_import_feedback(
                        loc,
                        outcome.imported_count,
                        &outcome.warnings,
                        &outcome.skipped_remote,
                    );
                    set_file.set(outcome.file);
                    set_feedback.set(Some(msg));
                    import_json.set(String::new());
                }
                Err(e) => set_feedback.set(Some(e)),
            }
        });
    };

    let auto_apply_after_paste = move |_| {
        spawn_local(async move {
            TimeoutFuture::new(0).await;
            let text = import_json.get_untracked();
            if !looks_like_mcp_json(&text) {
                return;
            }
            let loc = locale.get_untracked();
            match post_import_json(&text, loc).await {
                Ok(outcome) => {
                    let mut msg = format_import_feedback(
                        loc,
                        outcome.imported_count,
                        &outcome.warnings,
                        &outcome.skipped_remote,
                    );
                    msg.push('\n');
                    msg.push_str(i18n::settings_mcp_import_auto_paste(loc));
                    set_file.set(outcome.file);
                    set_feedback.set(Some(msg));
                    import_json.set(String::new());
                }
                Err(e) => set_feedback.set(Some(e)),
            }
        });
    };

    view! {
        <div class="settings-block settings-mcp-import" data-testid="settings-mcp-import-panel">
            <h3 class="settings-block-title">{move || i18n::settings_mcp_import_title(locale.get())}</h3>
            <p class="settings-intro">{move || i18n::settings_mcp_import_hint(locale.get())}</p>
            <textarea
                class="settings-input settings-mcp-import-json"
                rows="16"
                data-testid="settings-mcp-import-json"
                prop:placeholder=move || i18n::settings_mcp_import_placeholder(locale.get())
                prop:value=move || import_json.get()
                on:input=move |ev| {
                    if let Some(v) = event_textarea_value(&ev) {
                        import_json.set(v);
                    }
                }
                on:paste=auto_apply_after_paste
            ></textarea>
            <button
                type="button"
                class="settings-btn settings-btn-secondary"
                data-testid="settings-mcp-import-apply"
                on:click=apply_import
            >
                {move || i18n::settings_mcp_import_apply(locale.get())}
            </button>
        </div>
    }
}

fn event_textarea_value(ev: &leptos::ev::Event) -> Option<String> {
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlTextAreaElement>().ok())
        .map(|el| el.value())
}
