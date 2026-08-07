//! 设置页「工具」分区内的 GitHub Device Flow 连接块。

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{
    GithubDeviceStartDto, delete_secret_github, fetch_github_oauth_device_status,
    fetch_secrets_status, post_github_oauth_device_cancel, post_github_oauth_device_start,
};
use crate::i18n::{self, Locale};
use crate::tauri_shell::tauri_open_external_url;

#[derive(Clone, Copy)]
struct GithubUiSignals {
    github_set: RwSignal<bool>,
    github_suffix: RwSignal<Option<String>>,
    user_code: RwSignal<Option<String>>,
    verify_url: RwSignal<Option<String>>,
    status_line: RwSignal<Option<String>>,
    busy: RwSignal<bool>,
    err: RwSignal<Option<String>>,
}

fn refresh_github_secret_slot(loc: Locale, ui: GithubUiSignals) {
    spawn_local(async move {
        if let Ok(st) = fetch_secrets_status(loc).await {
            ui.github_set.set(st.github.set);
            ui.github_suffix.set(st.github.suffix.clone());
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
            refresh_github_secret_slot(loc, ui);
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

fn connection_label(loc: Locale, set: bool, suffix: Option<String>) -> String {
    if set {
        let suf = suffix.unwrap_or_else(|| "****".into());
        i18n::settings_github_connected(loc, &suf)
    } else {
        i18n::settings_github_disconnected(loc).to_string()
    }
}

fn disconnect_disabled(busy: bool, connected: bool) -> bool {
    busy || !connected
}

fn reopen_disabled(busy: bool, verify_url: Option<String>) -> bool {
    busy || verify_url.is_none()
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
    on_connect: Callback<()>,
    on_disconnect: Callback<()>,
    on_reopen: Callback<()>,
) -> impl IntoView {
    view! {
        <div class="settings-block" data-testid="settings-github-block">
            <h3 class="settings-block-title">{move || i18n::settings_github_block_title(locale.get())}</h3>
            <p class="settings-hint">{move || i18n::settings_github_block_hint(locale.get())}</p>
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
pub(crate) fn SettingsGithubBlock(locale: RwSignal<Locale>) -> impl IntoView {
    let ui = GithubUiSignals {
        github_set: RwSignal::new(false),
        github_suffix: RwSignal::new(None),
        user_code: RwSignal::new(None),
        verify_url: RwSignal::new(None),
        status_line: RwSignal::new(None),
        busy: RwSignal::new(false),
        err: RwSignal::new(None),
    };

    Effect::new(move |_| {
        let _ = locale.get();
        refresh_github_secret_slot(locale.get_untracked(), ui);
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
            on_connect=on_connect
            on_disconnect=on_disconnect
            on_reopen=on_reopen
        />
    }
}
