//! 设置页「工具」分区内的 GitHub Device Flow 连接块（含钥匙串 Client ID）。

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::event_target_value;

use crate::api::{
    GithubDeviceStartDto, delete_secret_github, delete_secret_github_oauth_client_id,
    fetch_github_oauth_device_status, fetch_secrets_status, post_github_oauth_device_cancel,
    post_github_oauth_device_start, put_secret_github_oauth_client_id,
};
use crate::i18n::{self, Locale};
use crate::tauri_shell::tauri_open_external_url;

#[derive(Clone, Copy)]
struct GithubUiSignals {
    github_set: RwSignal<bool>,
    github_suffix: RwSignal<Option<String>>,
    client_id_set: RwSignal<bool>,
    client_id_suffix: RwSignal<Option<String>>,
    client_id_env: RwSignal<bool>,
    client_id_draft: RwSignal<String>,
    client_id_feedback: RwSignal<Option<String>>,
    user_code: RwSignal<Option<String>>,
    verify_url: RwSignal<Option<String>>,
    status_line: RwSignal<Option<String>>,
    busy: RwSignal<bool>,
    err: RwSignal<Option<String>>,
}

fn secret_suffix_preview(raw: &str) -> String {
    let s = raw.trim();
    if s.chars().count() >= 4 {
        s.chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    } else {
        "****".into()
    }
}

fn refresh_github_secret_slots(loc: Locale, ui: GithubUiSignals, report_err: bool) {
    spawn_local(async move {
        match fetch_secrets_status(loc).await {
            Ok(st) => {
                ui.github_set.set(st.github.set);
                ui.github_suffix.set(st.github.suffix.clone());
                ui.client_id_set.set(st.github_oauth_client_id.set);
                ui.client_id_suffix
                    .set(st.github_oauth_client_id.suffix.clone());
                ui.client_id_env.set(st.github_oauth_client_id_env);
            }
            Err(e) if report_err => ui.err.set(Some(e)),
            Err(_) => {}
        }
    });
}

fn apply_terminal_device_state(
    loc: Locale,
    state: &str,
    error: Option<String>,
    ui: GithubUiSignals,
) {
    match state {
        "success" => {
            ui.err.set(None);
            refresh_github_secret_slots(loc, ui, true);
            ui.busy.set(false);
        }
        "denied" | "expired" | "cancelled" | "error" => {
            ui.err.set(Some(
                error.unwrap_or_else(|| i18n::settings_github_device_state(loc, state)),
            ));
            ui.busy.set(false);
        }
        _ => {}
    }
}

fn is_device_terminal(state: &str) -> bool {
    matches!(
        state,
        "success" | "denied" | "expired" | "cancelled" | "error"
    )
}

async fn poll_until_device_done(loc: Locale, start: GithubDeviceStartDto, ui: GithubUiSignals) {
    let interval_ms = u32::try_from(start.interval.max(1).saturating_mul(1000)).unwrap_or(5000);
    let expires = start.expires_in.max(60);
    let mut waited = 0u64;
    loop {
        TimeoutFuture::new(interval_ms).await;
        waited = waited.saturating_add(start.interval.max(1));
        match fetch_github_oauth_device_status(loc).await {
            Ok(st) => {
                ui.status_line
                    .set(Some(i18n::settings_github_device_state(loc, &st.state)));
                if is_device_terminal(&st.state) {
                    apply_terminal_device_state(loc, &st.state, st.error, ui);
                    return;
                }
            }
            Err(e) => {
                ui.err.set(Some(e));
                ui.busy.set(false);
                return;
            }
        }
        if waited >= expires {
            ui.err
                .set(Some(i18n::settings_github_device_expired(loc).to_string()));
            ui.busy.set(false);
            return;
        }
    }
}

fn spawn_device_connect(loc: Locale, ui: GithubUiSignals) {
    if ui.busy.get_untracked() {
        return;
    }
    ui.busy.set(true);
    ui.err.set(None);
    ui.status_line.set(None);
    ui.user_code.set(None);
    ui.verify_url.set(None);
    spawn_local(async move {
        match post_github_oauth_device_start(loc).await {
            Ok(start) => {
                ui.user_code.set(Some(start.user_code.clone()));
                ui.verify_url
                    .set(Some(start.verification_uri_complete.clone()));
                tauri_open_external_url(&start.verification_uri_complete);
                poll_until_device_done(loc, start, ui).await;
            }
            Err(e) => {
                ui.err.set(Some(e));
                ui.busy.set(false);
            }
        }
    });
}

fn spawn_device_disconnect(loc: Locale, ui: GithubUiSignals) {
    if ui.busy.get_untracked() {
        return;
    }
    ui.busy.set(true);
    spawn_local(async move {
        let _ = post_github_oauth_device_cancel(loc).await;
        let _ = delete_secret_github(loc).await;
        ui.github_set.set(false);
        ui.github_suffix.set(None);
        ui.user_code.set(None);
        ui.verify_url.set(None);
        ui.status_line.set(None);
        ui.busy.set(false);
    });
}

fn spawn_save_client_id(loc: Locale, ui: GithubUiSignals) {
    if ui.busy.get_untracked() {
        return;
    }
    let draft = ui.client_id_draft.get_untracked();
    ui.busy.set(true);
    ui.err.set(None);
    ui.client_id_feedback.set(None);
    spawn_local(async move {
        match put_secret_github_oauth_client_id(&draft, loc).await {
            Ok(()) => {
                ui.client_id_draft.set(String::new());
                if draft.trim().is_empty() {
                    ui.client_id_set.set(false);
                    ui.client_id_suffix.set(None);
                    ui.client_id_feedback
                        .set(Some(i18n::settings_github_client_id_cleared(loc).into()));
                } else {
                    ui.client_id_set.set(true);
                    ui.client_id_suffix.set(Some(secret_suffix_preview(&draft)));
                    ui.client_id_feedback
                        .set(Some(i18n::settings_github_client_id_saved(loc).into()));
                }
                refresh_github_secret_slots(loc, ui, true);
            }
            Err(e) => ui.err.set(Some(e)),
        }
        ui.busy.set(false);
    });
}

fn spawn_clear_client_id(loc: Locale, ui: GithubUiSignals) {
    if ui.busy.get_untracked() {
        return;
    }
    ui.busy.set(true);
    ui.err.set(None);
    ui.client_id_feedback.set(None);
    spawn_local(async move {
        match delete_secret_github_oauth_client_id(loc).await {
            Ok(()) => {
                ui.client_id_draft.set(String::new());
                ui.client_id_set.set(false);
                ui.client_id_suffix.set(None);
                ui.client_id_feedback
                    .set(Some(i18n::settings_github_client_id_cleared(loc).into()));
                refresh_github_secret_slots(loc, ui, true);
            }
            Err(e) => ui.err.set(Some(e)),
        }
        ui.busy.set(false);
    });
}

fn connection_label(loc: Locale, set: bool, suffix: Option<String>) -> String {
    if set {
        let suf = suffix.unwrap_or_else(|| "****".into());
        i18n::settings_github_connected(loc, &suf)
    } else {
        i18n::settings_github_disconnected(loc).to_string()
    }
}

fn client_id_keychain_label(loc: Locale, set: bool, suffix: Option<String>) -> String {
    if set {
        let suf = suffix.unwrap_or_else(|| "****".into());
        i18n::settings_github_client_id_set(loc, &suf)
    } else {
        i18n::settings_github_client_id_unset(loc).to_string()
    }
}

fn disconnect_disabled(busy: bool, connected: bool) -> bool {
    busy || !connected
}

fn reopen_disabled(busy: bool, verify_url: Option<String>) -> bool {
    busy || verify_url.is_none()
}

fn clear_client_id_disabled(busy: bool, set: bool) -> bool {
    busy || !set
}

fn client_id_input_placeholder(set: bool) -> &'static str {
    if set { "••••••••" } else { "" }
}

#[component]
fn SettingsGithubClientIdStatus(locale: RwSignal<Locale>, ui: GithubUiSignals) -> impl IntoView {
    view! {
        <Show when=move || ui.client_id_env.get()>
            <p class="settings-hint" data-testid="settings-github-client-id-env">
                {move || i18n::settings_github_client_id_env_overrides(locale.get())}
            </p>
        </Show>
        <p class="settings-hint" data-testid="settings-github-client-id-status">
            {move || {
                client_id_keychain_label(
                    locale.get(),
                    ui.client_id_set.get(),
                    ui.client_id_suffix.get(),
                )
            }}
        </p>
    }
}

#[component]
fn SettingsGithubClientIdActions(locale: RwSignal<Locale>, ui: GithubUiSignals) -> impl IntoView {
    view! {
        <div class="settings-row">
            <button
                type="button"
                class="btn btn-secondary btn-sm"
                data-testid="settings-github-client-id-save"
                prop:disabled=move || ui.busy.get()
                on:click=move |_| spawn_save_client_id(locale.get_untracked(), ui)
            >
                {move || i18n::settings_github_client_id_save(locale.get())}
            </button>
            <button
                type="button"
                class="btn btn-ghost btn-sm"
                data-testid="settings-github-client-id-clear"
                prop:disabled=move || {
                    clear_client_id_disabled(ui.busy.get(), ui.client_id_set.get())
                }
                on:click=move |_| spawn_clear_client_id(locale.get_untracked(), ui)
            >
                {move || i18n::settings_github_client_id_clear(locale.get())}
            </button>
        </div>
        <Show when=move || ui.client_id_feedback.get().is_some()>
            <p class="settings-hint" data-testid="settings-github-client-id-feedback">
                {move || ui.client_id_feedback.get().unwrap_or_default()}
            </p>
        </Show>
    }
}

#[component]
fn SettingsGithubClientIdBlock(
    locale: RwSignal<Locale>,
    ui: GithubUiSignals,
    input_id: &'static str,
) -> impl IntoView {
    view! {
        <div class="settings-field" data-testid="settings-github-client-id">
            <label class="settings-field-label" for=input_id>
                {move || i18n::settings_github_client_id_label(locale.get())}
            </label>
            <p class="settings-hint">{move || i18n::settings_github_client_id_hint(locale.get())}</p>
            <SettingsGithubClientIdStatus locale=locale ui=ui />
            <input
                id=input_id
                class="input"
                type="text"
                autocomplete="off"
                spellcheck="false"
                data-testid="settings-github-client-id-input"
                prop:value=move || ui.client_id_draft.get()
                prop:placeholder=move || client_id_input_placeholder(ui.client_id_set.get())
                prop:disabled=move || ui.busy.get()
                on:input=move |ev| ui.client_id_draft.set(event_target_value(&ev))
            />
            <SettingsGithubClientIdActions locale=locale ui=ui />
        </div>
    }
}

#[component]
fn SettingsGithubBlockActions(
    locale: RwSignal<Locale>,
    ui: GithubUiSignals,
    on_connect: Callback<()>,
    on_disconnect: Callback<()>,
    on_reopen: Callback<()>,
) -> impl IntoView {
    view! {
        <div class="settings-row">
            <button
                type="button"
                class="btn btn-primary btn-sm"
                data-testid="settings-github-connect"
                prop:disabled=move || ui.busy.get()
                on:click=move |_| on_connect.run(())
            >
                {move || i18n::settings_github_connect(locale.get())}
            </button>
            <button
                type="button"
                class="btn btn-secondary btn-sm"
                data-testid="settings-github-disconnect"
                prop:disabled=move || disconnect_disabled(ui.busy.get(), ui.github_set.get())
                on:click=move |_| on_disconnect.run(())
            >
                {move || i18n::settings_github_disconnect(locale.get())}
            </button>
            <button
                type="button"
                class="btn btn-ghost btn-sm"
                data-testid="settings-github-reopen"
                prop:disabled=move || reopen_disabled(ui.busy.get(), ui.verify_url.get())
                on:click=move |_| on_reopen.run(())
            >
                {move || i18n::settings_github_reopen(locale.get())}
            </button>
        </div>
    }
}

#[component]
fn SettingsGithubBlockView(
    locale: RwSignal<Locale>,
    ui: GithubUiSignals,
    input_id: &'static str,
    on_connect: Callback<()>,
    on_disconnect: Callback<()>,
    on_reopen: Callback<()>,
) -> impl IntoView {
    view! {
        <div class="settings-block" data-testid="settings-github-block">
            <h3 class="settings-block-title">{move || i18n::settings_github_block_title(locale.get())}</h3>
            <p class="settings-hint">{move || i18n::settings_github_block_hint(locale.get())}</p>
            <SettingsGithubClientIdBlock locale=locale ui=ui input_id=input_id />
            <p class="settings-hint">
                {move || connection_label(locale.get(), ui.github_set.get(), ui.github_suffix.get())}
            </p>
            <Show when=move || ui.user_code.get().is_some()>
                <p class="settings-github-user-code" data-testid="settings-github-user-code">
                    {move || ui.user_code.get().unwrap_or_default()}
                </p>
            </Show>
            <Show when=move || ui.status_line.get().is_some()>
                <p class="settings-hint" data-testid="settings-github-status">
                    {move || ui.status_line.get().unwrap_or_default()}
                </p>
            </Show>
            <Show when=move || ui.err.get().is_some()>
                <p class="settings-error" data-testid="settings-github-error">
                    {move || ui.err.get().unwrap_or_default()}
                </p>
            </Show>
            <SettingsGithubBlockActions
                locale=locale
                ui=ui
                on_connect=on_connect
                on_disconnect=on_disconnect
                on_reopen=on_reopen
            />
        </div>
    }
}

#[component]
pub(crate) fn SettingsGithubBlock(
    locale: RwSignal<Locale>,
    /// `<input id=…>`：设置页与弹窗可能同时挂载，须用不同 id。
    input_id: &'static str,
) -> impl IntoView {
    let ui = GithubUiSignals {
        github_set: RwSignal::new(false),
        github_suffix: RwSignal::new(None),
        client_id_set: RwSignal::new(false),
        client_id_suffix: RwSignal::new(None),
        client_id_env: RwSignal::new(false),
        client_id_draft: RwSignal::new(String::new()),
        client_id_feedback: RwSignal::new(None),
        user_code: RwSignal::new(None),
        verify_url: RwSignal::new(None),
        status_line: RwSignal::new(None),
        busy: RwSignal::new(false),
        err: RwSignal::new(None),
    };

    Effect::new(move |_| {
        let _ = locale.get();
        refresh_github_secret_slots(locale.get_untracked(), ui, false);
    });

    let on_connect = Callback::new(move |_| {
        spawn_device_connect(locale.get_untracked(), ui);
    });
    let on_disconnect = Callback::new(move |_| {
        spawn_device_disconnect(locale.get_untracked(), ui);
    });
    let on_reopen = Callback::new(move |_| {
        if let Some(url) = ui.verify_url.get_untracked() {
            tauri_open_external_url(&url);
        }
    });

    view! {
        <SettingsGithubBlockView
            locale=locale
            ui=ui
            input_id=input_id
            on_connect=on_connect
            on_disconnect=on_disconnect
            on_reopen=on_reopen
        />
    }
}
