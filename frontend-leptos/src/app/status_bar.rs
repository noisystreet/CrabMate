//! 底栏状态：模型、base_url、角色、运行态。

use std::sync::Arc;

use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;

use crate::api::StatusData;
use crate::api::load_client_llm_text_fields_from_storage;
use crate::app_prefs::{status_bar_effective_api_base, status_bar_effective_model};
use crate::i18n::{self, Locale};

#[component]
fn StatusFetchErrorPanel(
    fetch_err: String,
    refresh_status: Arc<dyn Fn() + Send + Sync>,
    locale: RwSignal<Locale>,
) -> impl IntoView {
    let fetch_err_for_title = fetch_err.clone();
    let fetch_err_for_body = fetch_err;
    view! {
        <div
            class="status-fetch-error"
            role="status"
            aria-live="polite"
        >
            <span class="status-fetch-error-text" title=fetch_err_for_title.clone()>
                {move || i18n::status_fetch_error(locale.get(), fetch_err_for_body.as_str())}
            </span>
            <button
                type="button"
                class="btn btn-secondary btn-sm"
                on:click=move |_| refresh_status()
            >
                {move || i18n::status_retry(locale.get())}
            </button>
        </div>
    }
}

#[component]
#[allow(clippy::too_many_arguments)]
fn StatusBarFooterBody(
    status_fetch_err: RwSignal<Option<String>>,
    status_err: RwSignal<Option<String>>,
    tool_busy: RwSignal<bool>,
    status_busy: RwSignal<bool>,
    status_loading: RwSignal<bool>,
    status_data: RwSignal<Option<StatusData>>,
    client_llm_storage_tick: RwSignal<u64>,
    selected_agent_role: RwSignal<Option<String>>,
    refresh_status: Arc<dyn Fn() + Send + Sync>,
    locale: RwSignal<Locale>,
) -> impl IntoView {
    view! {
        <footer class=move || {
            if status_fetch_err.get().is_some() {
                "status-bar status-bar-fetch-error"
            } else {
                "status-bar"
            }
        }>
            <div class="status-chips">
                {move || {
                    if status_loading.get() {
                        view! {
                            <div
                                class="status-chips-skeleton"
                                aria-busy="true"
                                prop:aria-label=move || i18n::status_loading_aria(locale.get())
                            >
                                <span class="status-chip status-chip-skeleton">
                                    <span class="skeleton skeleton-chip-label"></span>
                                    <span class="skeleton skeleton-chip-value skeleton-chip-model"></span>
                                </span>
                                <span class="status-chip status-chip-skeleton status-chip-url">
                                    <span class="skeleton skeleton-chip-label"></span>
                                    <span class="skeleton skeleton-chip-value skeleton-chip-url-bar"></span>
                                </span>
                                <span class="status-chip status-chip-skeleton status-chip-role">
                                    <span class="skeleton skeleton-chip-label"></span>
                                    <span class="skeleton skeleton-chip-value skeleton-chip-role-select"></span>
                                </span>
                            </div>
                        }
                        .into_any()
                    } else if let Some(fetch_err) = status_fetch_err.get() {
                        view! {
                            <StatusFetchErrorPanel
                                fetch_err=fetch_err
                                refresh_status=refresh_status.clone()
                                locale=locale
                            />
                        }
                        .into_any()
                    } else {
                        view! {
                            <>
                                <span class="status-chip">
                                    <span class="status-chip-label">
                                        {move || i18n::status_chip_model(locale.get())}
                                    </span>
                                    <span class="status-chip-value">{move || {
                                        let _tick = client_llm_storage_tick.get();
                                        let sd = status_data.get();
                                        let (_, stored_model) =
                                            load_client_llm_text_fields_from_storage();
                                        status_bar_effective_model(
                                            sd.as_ref(),
                                            stored_model.as_str(),
                                        )
                                    }}</span>
                                </span>
                                <span class="status-chip status-chip-url" title=move || {
                                    let _tick = client_llm_storage_tick.get();
                                    let sd = status_data.get();
                                    let (stored_base, _) =
                                        load_client_llm_text_fields_from_storage();
                                    status_bar_effective_api_base(
                                        sd.as_ref(),
                                        stored_base.as_str(),
                                    )
                                }>
                                    <span class="status-chip-label">
                                        {move || i18n::status_chip_base_url(locale.get())}
                                    </span>
                                    <span class="status-chip-value">{move || {
                                        let _tick = client_llm_storage_tick.get();
                                        let sd = status_data.get();
                                        let (stored_base, _stored_model) =
                                            load_client_llm_text_fields_from_storage();
                                        status_bar_effective_api_base(
                                            sd.as_ref(),
                                            stored_base.as_str(),
                                        )
                                    }}</span>
                                </span>
                                <label
                                    class="status-chip status-chip-role"
                                    prop:title=move || i18n::status_role_title_attr(locale.get())
                                >
                                    <span class="status-chip-label">
                                        {move || i18n::status_role_label(locale.get())}
                                    </span>
                                    <select
                                        class="status-agent-select"
                                        prop:value=move || {
                                            selected_agent_role
                                                .get()
                                                .unwrap_or_else(|| "__default__".to_string())
                                        }
                                        on:change=move |ev| {
                                            let v = event_target_value(&ev);
                                            let t = v.trim();
                                            if t.is_empty() || t == "__default__" {
                                                selected_agent_role.set(None);
                                            } else {
                                                selected_agent_role.set(Some(t.to_string()));
                                            }
                                        }
                                    >
                                        <option value="__default__">{move || {
                                            let loc = locale.get();
                                            match status_data
                                                .get()
                                                .and_then(|d| d.default_agent_role_id.clone())
                                            {
                                                Some(id) => {
                                                    i18n::status_default_option(loc, Some(id.as_str()))
                                                }
                                                None => i18n::status_default_option(loc, None),
                                            }
                                        }}</option>
                                        {move || {
                                            status_data
                                                .get()
                                                .map(|d| d.agent_role_ids)
                                                .unwrap_or_default()
                                                .into_iter()
                                                .map(|id| {
                                                    let label = id.clone();
                                                    view! { <option value=id>{label}</option> }
                                                })
                                                .collect_view()
                                        }}
                                    </select>
                                </label>
                            </>
                        }
                        .into_any()
                    }
                }}
            </div>
            <span class=move || {
                let kind = if status_fetch_err.get().is_some() || status_err.get().is_some() {
                    "error"
                } else if tool_busy.get() {
                    "tool"
                } else if status_busy.get() {
                    "running"
                } else {
                    "ready"
                };
                format!("status-run status-run-{kind}")
            }>
                <span class="status-run-dot" aria-hidden="true"></span>
                <span>{move || {
                    let loc = locale.get();
                    if status_fetch_err.get().is_some() {
                        i18n::status_unavailable(loc).to_string()
                    } else if let Some(e) = status_err.get() {
                        format!("{}{e}", i18n::status_error_prefix(loc))
                    } else if tool_busy.get() {
                        i18n::status_tool_running(loc).to_string()
                    } else if status_busy.get() {
                        i18n::status_model_running(loc).to_string()
                    } else {
                        i18n::status_ready(loc).to_string()
                    }
                }}</span>
            </span>
        </footer>
    }
}

#[allow(clippy::too_many_arguments)]
pub fn status_bar_footer_view(
    status_bar_visible: RwSignal<bool>,
    status_fetch_err: RwSignal<Option<String>>,
    status_err: RwSignal<Option<String>>,
    tool_busy: RwSignal<bool>,
    status_busy: RwSignal<bool>,
    status_loading: RwSignal<bool>,
    status_data: RwSignal<Option<StatusData>>,
    client_llm_storage_tick: RwSignal<u64>,
    selected_agent_role: RwSignal<Option<String>>,
    refresh_status: Arc<dyn Fn() + Send + Sync>,
    locale: RwSignal<Locale>,
) -> impl IntoView {
    view! {
        <Show when=move || status_bar_visible.get()>
            <StatusBarFooterBody
                status_fetch_err=status_fetch_err
                status_err=status_err
                tool_busy=tool_busy
                status_busy=status_busy
                status_loading=status_loading
                status_data=status_data
                client_llm_storage_tick=client_llm_storage_tick
                selected_agent_role=selected_agent_role
                refresh_status=refresh_status.clone()
                locale=locale
            />
        </Show>
    }
}
