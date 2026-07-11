//! 底栏状态：模型、base_url、角色、运行态。

use std::sync::Arc;

use leptos::prelude::*;

use crate::api::load_client_llm_text_fields_from_storage;
use crate::app_prefs::{
    status_bar_effective_api_base, status_bar_effective_llm_context_tokens,
    status_bar_effective_model, status_bar_new_session_baseline_prompt_tokens,
};
use crate::chat_session_state::{ChatSessionSignals, ChatStreamBusyMemos};
use crate::i18n::{self, Locale};

use super::app_shell_ctx::StatusBarFooterSignals;
use super::shell_runtime_context::expect_chat_shell_ctx;
use super::status_agent_role_menu::{AgentRoleMenuProps, StatusAgentRoleMenu};
use super::status_fetch_state::status_bar_should_show_skeleton;
use super::status_tasks_state::StatusTasksSignals;

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
    agent_role_user_override: RwSignal<bool>,
    locale: RwSignal<Locale>,
}

/// 有活动会话即展示「上下文」芯片；用量在水合完成前显示 `— / 上限`。
fn status_bar_context_chip_visible(chat: ChatSessionSignals) -> bool {
    !chat.active_id.get().is_empty()
}

fn status_bar_context_used_for_active_session(chat: ChatSessionSignals) -> Option<u32> {
    let snap = chat.conversation_prompt_tokens.get()?;
    let aid = chat.active_id.get_untracked();
    let cid_matches = chat.sessions.with_untracked(|list| {
        list.iter().find(|s| s.id == aid).and_then(|s| {
            s.trimmed_server_conversation_id()
                .map(|c| c == snap.conversation_id.as_str())
        })
    });
    if cid_matches != Some(true) {
        return None;
    }
    snap.tiktoken.as_ref().map(|t| t.prompt_tokens)
}

fn active_session_has_server_conversation_id(chat: ChatSessionSignals) -> bool {
    let aid = chat.active_id.get_untracked();
    chat.sessions.with_untracked(|list| {
        list.iter()
            .find(|s| s.id == aid)
            .and_then(|s| s.trimmed_server_conversation_id())
            .is_some()
    })
}

fn status_bar_context_effective_used(
    chat: ChatSessionSignals,
    st: StatusTasksSignals,
    selected_agent_role: Option<&str>,
) -> Option<u32> {
    if let Some(n) = status_bar_context_used_for_active_session(chat) {
        return Some(n);
    }
    if active_session_has_server_conversation_id(chat) {
        return None;
    }
    let role = selected_agent_role.map(str::trim).filter(|s| !s.is_empty());
    status_bar_new_session_baseline_prompt_tokens(st.status_data.get().as_ref(), role)
}

/// 新会话、尚无服务端实测 tiktoken 时，展示 `GET /status` 的 system 基线（带 `~` 前缀）。
fn status_bar_context_is_baseline_estimate(chat: ChatSessionSignals, used: Option<u32>) -> bool {
    used.is_some()
        && status_bar_context_used_for_active_session(chat).is_none()
        && !active_session_has_server_conversation_id(chat)
}

fn status_bar_context_cap_and_used(
    chat: ChatSessionSignals,
    st: StatusTasksSignals,
    client_llm_storage_tick: RwSignal<u64>,
    selected_agent_role: RwSignal<Option<String>>,
) -> (u32, Option<u32>) {
    let _tick = client_llm_storage_tick.get();
    let sd = st.status_data.get();
    let (_, _, _, stored_ctx, _) = load_client_llm_text_fields_from_storage();
    let cap = status_bar_effective_llm_context_tokens(sd.as_ref(), stored_ctx.as_str());
    let role_sel = selected_agent_role.get();
    let role = role_sel.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let used = status_bar_context_effective_used(chat, st, role);
    (cap, used)
}

fn status_bar_context_value_text(cap: u32, used: Option<u32>, baseline_estimate: bool) -> String {
    let prefix = if baseline_estimate { "~" } else { "" };
    match (used, cap > 0) {
        (Some(u), true) => {
            let pct = (u as f64 / cap as f64) * 100.0;
            format!("{prefix}{u} / {cap} ({:.1}%)", pct.min(999.9))
        }
        (Some(u), false) => format!("{prefix}{u}"),
        (None, true) => format!("— / {cap}"),
        (None, false) => "—".to_string(),
    }
}

#[component]
fn StatusBarContextChip(
    st: StatusTasksSignals,
    chat: ChatSessionSignals,
    client_llm_storage_tick: RwSignal<u64>,
    selected_agent_role: RwSignal<Option<String>>,
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
                            selected_agent_role,
                        );
                        let baseline = status_bar_context_is_baseline_estimate(chat, used);
                        status_bar_context_value_text(cap, used, baseline)
                    }}</span>
                </span>
                <Show when=move || {
                    let (cap, used) = status_bar_context_cap_and_used(
                        chat,
                        st,
                        client_llm_storage_tick,
                        selected_agent_role,
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
                                selected_agent_role,
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
                                selected_agent_role,
                            );
                            let u = used.unwrap_or(0);
                            let pct = (u as f64 / cap as f64) * 100.0;
                            let baseline =
                                status_bar_context_is_baseline_estimate(chat, used);
                            if baseline {
                                "status-context-meter-fill status-context-meter-fill--estimate".to_string()
                            } else if pct >= 90.0 {
                                "status-context-meter-fill status-context-meter-fill--warn".to_string()
                            } else {
                                "status-context-meter-fill".to_string()
                            }
                        }></div>
                    </div>
                </Show>
            </span>
        </Show>
    }
}

#[component]
fn StatusBarGithubChip(st: StatusTasksSignals, locale: RwSignal<Locale>) -> impl IntoView {
    view! {
        <Show when=move || {
            st.github_checks
                .get()
                .and_then(|c| c.pr_number)
                .is_some()
        }>
            <span
                class="status-chip status-chip-github"
                prop:title=move || {
                    st.github_checks
                        .get()
                        .and_then(|c| c.pr_title.clone())
                        .unwrap_or_default()
                }
            >
                <span class="status-chip-label">
                    {move || i18n::github_status_chip_label(locale.get())}
                </span>
                <span class="status-chip-value">{move || {
                    let c = st.github_checks.get().unwrap_or_default();
                    let n = c.pr_number.unwrap_or(0);
                    let title = c.pr_title.as_deref().unwrap_or("");
                    let chip = i18n::github_status_chip_pr(locale.get(), n, title);
                    let s = c.summary;
                    format!(
                        "{} · {}",
                        chip,
                        i18n::github_checks_summary(locale.get(), s.passing, s.failing, s.pending)
                    )
                }}</span>
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
            <span class="status-chip status-chip-skeleton status-chip-context">
                <span class="skeleton skeleton-chip-label"></span>
                <span class="skeleton skeleton-context-bar"></span>
            </span>
        </div>
    }
}

#[component]
fn StatusBarChipsLoaded(
    st: StatusTasksSignals,
    client_llm_storage_tick: RwSignal<u64>,
    selected_agent_role: RwSignal<Option<String>>,
    agent_role_user_override: RwSignal<bool>,
    locale: RwSignal<Locale>,
    role_menu_open: RwSignal<bool>,
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
            <span
                class="status-chip status-chip-role"
                prop:title=move || i18n::status_role_title_attr(locale.get())
            >
                <span class="status-chip-label">
                    {move || i18n::status_role_label(locale.get())}
                </span>
                <StatusAgentRoleMenu props=AgentRoleMenuProps {
                    st,
                    locale,
                    chat,
                    selected_agent_role,
                    agent_role_user_override,
                    menu_open: role_menu_open,
                } />
            </span>
            <StatusBarContextChip
                st=st
                chat=chat
                client_llm_storage_tick=client_llm_storage_tick
                selected_agent_role=selected_agent_role
                locale=locale
            />
            <StatusBarGithubChip st=st locale=locale />
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
        agent_role_user_override,
        locale,
    } = chips;
    let role_menu_open = RwSignal::new(false);
    view! {
        <div
            class="status-chips"
            class:status-chips--role-menu-open=move || role_menu_open.get()
        >
            {move || {
                let phase = st.status_fetch_phase.get();
                let has_data = st.status_data.get().is_some();
                let has_error = st.status_fetch_err.get().is_some();
                if status_bar_should_show_skeleton(phase, has_data, has_error) {
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
                            agent_role_user_override=agent_role_user_override
                            locale=locale
                            role_menu_open=role_menu_open
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
    locale: RwSignal<Locale>,
) -> impl IntoView {
    view! {
        <span class=move || {
            let kind = if st.status_fetch_err.get().is_some() || status_err.get().is_some() {
                "error"
            } else if stream_busy_memos.tool_timeline_busy_ui.get() {
                "tool"
            } else if stream_busy_memos.model_status_busy.get() {
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
                } else if stream_busy_memos.model_status_busy.get() {
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
    client_llm_storage_tick: RwSignal<u64>,
    selected_agent_role: RwSignal<Option<String>>,
    agent_role_user_override: RwSignal<bool>,
    refresh_status: Arc<dyn Fn() + Send + Sync>,
) -> impl IntoView {
    let locale = expect_chat_shell_ctx().locale;
    let chips = StatusBarChipsSignals {
        st,
        client_llm_storage_tick,
        selected_agent_role,
        agent_role_user_override,
        locale,
    };
    view! {
        <footer
            data-testid="status-bar"
            class=move || {
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
        client_llm_storage_tick,
        selected_agent_role,
        agent_role_user_override,
        refresh_status,
    } = signals;
    view! {
        <Show when=move || status_bar_visible.get()>
            <StatusBarFooterBody
                st=st
                status_err=status_err
                stream_busy_memos=stream_busy_memos
                client_llm_storage_tick=client_llm_storage_tick
                selected_agent_role=selected_agent_role
                agent_role_user_override=agent_role_user_override
                refresh_status=refresh_status.clone()
            />
        </Show>
    }
}
