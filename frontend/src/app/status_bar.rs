//! 底栏状态：模型、base_url、角色、运行态。

use std::sync::Arc;

use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;

use crate::api::load_client_llm_text_fields_from_storage;
use crate::app_prefs::{
    status_bar_effective_api_base, status_bar_effective_llm_context_tokens,
    status_bar_effective_model,
};
use crate::chat_session_state::{ChatSessionSignals, ChatStreamBusyMemos};

use super::app_shell_ctx::StatusBarFooterSignals;
use super::shell_runtime_context::expect_chat_shell_ctx;
use super::status_tasks_state::StatusTasksSignals;
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

#[derive(Clone, Copy)]
struct StatusBarChipsSignals {
    st: StatusTasksSignals,
    client_llm_storage_tick: RwSignal<u64>,
    selected_agent_role: RwSignal<Option<String>>,
    locale: RwSignal<Locale>,
}

fn status_bar_context_chip_visible(chat: ChatSessionSignals) -> bool {
    let Some(snap) = chat.conversation_prompt_tokens.get() else {
        return false;
    };
    let aid = chat.active_id.get();
    chat.sessions.with(|list| {
        list.iter().find(|s| s.id == aid).is_some_and(|s| {
            s.trimmed_server_conversation_id()
                .is_some_and(|c| c == snap.conversation_id.as_str())
        })
    })
}

fn status_bar_context_cap_and_used(
    chat: ChatSessionSignals,
    st: StatusTasksSignals,
    client_llm_storage_tick: RwSignal<u64>,
) -> (u32, Option<u32>) {
    let _tick = client_llm_storage_tick.get();
    let sd = st.status_data.get();
    let (_, _, _, stored_ctx, _) = load_client_llm_text_fields_from_storage();
    let cap = status_bar_effective_llm_context_tokens(sd.as_ref(), stored_ctx.as_str());
    let used = chat
        .conversation_prompt_tokens
        .get()
        .and_then(|s| s.tiktoken.as_ref().map(|t| t.prompt_tokens));
    (cap, used)
}

fn status_bar_context_value_text(cap: u32, used: Option<u32>) -> String {
    match (used, cap > 0) {
        (Some(u), true) => {
            let pct = (u as f64 / cap as f64) * 100.0;
            format!("{u} / {cap} ({:.1}%)", pct.min(999.9))
        }
        (Some(u), false) => format!("{u}"),
        (None, true) => format!("— / {cap}"),
        (None, false) => "—".to_string(),
    }
}

#[component]
fn StatusBarContextChip(
    st: StatusTasksSignals,
    chat: ChatSessionSignals,
    client_llm_storage_tick: RwSignal<u64>,
    locale: RwSignal<Locale>,
) -> impl IntoView {
    view! {
        <Show when=move || status_bar_context_chip_visible(chat)>
            <span
                class="status-chip status-chip-context"
                prop:title=move || i18n::status_chip_context_tooltip(locale.get())
            >
                <span class="status-chip-context-row">
                    <span class="status-chip-label">
                        {move || i18n::status_chip_context(locale.get())}
                    </span>
                    <span class="status-chip-value">{move || {
                        let (cap, used) = status_bar_context_cap_and_used(
                            chat,
                            st,
                            client_llm_storage_tick,
                        );
                        status_bar_context_value_text(cap, used)
                    }}</span>
                </span>
                <Show when=move || {
                    let (cap, used) = status_bar_context_cap_and_used(
                        chat,
                        st,
                        client_llm_storage_tick,
                    );
                    cap > 0 && used.is_some()
                }>
                    <div
                        class="status-context-meter"
                        style=move || {
                            let (cap, used) = status_bar_context_cap_and_used(
                                chat,
                                st,
                                client_llm_storage_tick,
                            );
                            let u = used.unwrap_or(0);
                            let pct = ((u as f64 / cap as f64) * 100.0).min(100.0);
                            format!("--status-context-pct: {pct:.2}%")
                        }
                    >
                        <div class=move || {
                            let (cap, used) = status_bar_context_cap_and_used(
                                chat,
                                st,
                                client_llm_storage_tick,
                            );
                            let u = used.unwrap_or(0);
                            let pct = (u as f64 / cap as f64) * 100.0;
                            if pct >= 90.0 {
                                "status-context-meter-fill status-context-meter-fill--warn"
                            } else {
                                "status-context-meter-fill"
                            }
                        }></div>
                    </div>
                </Show>
            </span>
        </Show>
    }
}

#[component]
fn StatusBarChipsSkeleton(locale: RwSignal<Locale>) -> impl IntoView {
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
}

#[component]
fn StatusBarChipsLoaded(
    st: StatusTasksSignals,
    client_llm_storage_tick: RwSignal<u64>,
    selected_agent_role: RwSignal<Option<String>>,
    locale: RwSignal<Locale>,
) -> impl IntoView {
    let chat = expect_chat_shell_ctx().chat;
    view! {
        <>
            <span class="status-chip">
                <span class="status-chip-label">
                    {move || i18n::status_chip_model(locale.get())}
                </span>
                <span class="status-chip-value">{move || {
                    let _tick = client_llm_storage_tick.get();
                    let sd = st.status_data.get();
                    let (_, stored_model, _, _, _) = load_client_llm_text_fields_from_storage();
                    status_bar_effective_model(sd.as_ref(), stored_model.as_str())
                }}</span>
            </span>
            <span class="status-chip status-chip-url" title=move || {
                let _tick = client_llm_storage_tick.get();
                let sd = st.status_data.get();
                let (stored_base, _, _, _, _) = load_client_llm_text_fields_from_storage();
                status_bar_effective_api_base(sd.as_ref(), stored_base.as_str())
            }>
                <span class="status-chip-label">
                    {move || i18n::status_chip_base_url(locale.get())}
                </span>
                <span class="status-chip-value">{move || {
                    let _tick = client_llm_storage_tick.get();
                    let sd = st.status_data.get();
                    let (stored_base, _stored_model, _, _, _) =
                        load_client_llm_text_fields_from_storage();
                    status_bar_effective_api_base(sd.as_ref(), stored_base.as_str())
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
                        chat.clear_stream_resume_handles();
                    }
                >
                    <option value="__default__">{move || {
                        let loc = locale.get();
                        match st
                            .status_data
                            .get()
                            .and_then(|d| d.default_agent_role_id.clone())
                        {
                            Some(id) => i18n::status_default_option(loc, Some(id.as_str())),
                            None => i18n::status_default_option(loc, None),
                        }
                    }}</option>
                    {move || {
                        st.status_data
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
            <StatusBarContextChip
                st=st
                chat=chat
                client_llm_storage_tick=client_llm_storage_tick
                locale=locale
            />
        </>
    }
}

#[component]
fn StatusBarChipsRow(
    chips: StatusBarChipsSignals,
    refresh_status: Arc<dyn Fn() + Send + Sync>,
) -> impl IntoView {
    let StatusBarChipsSignals {
        st,
        client_llm_storage_tick,
        selected_agent_role,
        locale,
    } = chips;
    view! {
        <div class="status-chips">
            {move || {
                if st.status_loading.get() {
                    view! { <StatusBarChipsSkeleton locale=locale /> }.into_any()
                } else if let Some(fetch_err) = st.status_fetch_err.get() {
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
                        <StatusBarChipsLoaded
                            st=st
                            client_llm_storage_tick=client_llm_storage_tick
                            selected_agent_role=selected_agent_role
                            locale=locale
                        />
                    }
                    .into_any()
                }
            }}
        </div>
    }
}

#[component]
fn StatusBarRunIndicator(
    st: StatusTasksSignals,
    status_err: RwSignal<Option<String>>,
    stream_busy_memos: ChatStreamBusyMemos,
    status_busy: RwSignal<bool>,
    locale: RwSignal<Locale>,
) -> impl IntoView {
    view! {
        <span class=move || {
            let kind = if st.status_fetch_err.get().is_some() || status_err.get().is_some() {
                "error"
            } else if stream_busy_memos.tool_timeline_busy_ui.get() {
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
                if st.status_fetch_err.get().is_some() {
                    i18n::status_unavailable(loc).to_string()
                } else if let Some(e) = status_err.get() {
                    format!("{}{e}", i18n::status_error_prefix(loc))
                } else if stream_busy_memos.tool_timeline_busy_ui.get() {
                    i18n::status_tool_running(loc).to_string()
                } else if status_busy.get() {
                    i18n::status_model_running(loc).to_string()
                } else {
                    i18n::status_ready(loc).to_string()
                }
            }}</span>
        </span>
    }
}

#[component]
fn StatusBarFooterBody(
    st: StatusTasksSignals,
    status_err: RwSignal<Option<String>>,
    stream_busy_memos: ChatStreamBusyMemos,
    status_busy: RwSignal<bool>,
    client_llm_storage_tick: RwSignal<u64>,
    selected_agent_role: RwSignal<Option<String>>,
    refresh_status: Arc<dyn Fn() + Send + Sync>,
) -> impl IntoView {
    let locale = expect_chat_shell_ctx().locale;
    let chips = StatusBarChipsSignals {
        st,
        client_llm_storage_tick,
        selected_agent_role,
        locale,
    };
    view! {
        <footer class=move || {
            if st.status_fetch_err.get().is_some() {
                "status-bar status-bar-fetch-error"
            } else {
                "status-bar"
            }
        }>
            <StatusBarChipsRow chips=chips refresh_status=refresh_status />
            <StatusBarRunIndicator
                st=st
                status_err=status_err
                stream_busy_memos=stream_busy_memos
                status_busy=status_busy
                locale=locale
            />
        </footer>
    }
}

pub fn status_bar_footer_view(signals: StatusBarFooterSignals) -> impl IntoView {
    let StatusBarFooterSignals {
        status_bar_visible,
        status_tasks: st,
        status_err,
        stream_busy_memos,
        status_busy,
        client_llm_storage_tick,
        selected_agent_role,
        refresh_status,
    } = signals;
    view! {
        <Show when=move || status_bar_visible.get()>
            <StatusBarFooterBody
                st=st
                status_err=status_err
                stream_busy_memos=stream_busy_memos
                status_busy=status_busy
                client_llm_storage_tick=client_llm_storage_tick
                selected_agent_role=selected_agent_role
                refresh_status=refresh_status.clone()
            />
        </Show>
    }
}
