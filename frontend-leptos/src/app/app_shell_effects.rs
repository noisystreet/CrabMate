//! `App` 级副作用：首启加载会话、`localStorage` 与 DOM 偏好同步、`Escape` 分层关闭等。
//!
//! 从 `mod.rs` 抽出，使根组件以「声明信号 + 调用 `wire_*`」为主。

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::window_event_listener;
use wasm_bindgen::JsCast;

use crate::api::{
    client_llm_storage_has_api_key, fetch_web_ui_config, load_client_llm_text_fields_from_storage,
};
use crate::app_prefs::{
    AGENT_ROLE_KEY, BG_DECOR_KEY, SIDEBAR_RAIL_COLLAPSED_KEY, STATUS_BAR_VISIBLE_KEY,
    SidePanelView, TASKS_VISIBLE_KEY, THEME_KEY, WORKSPACE_VISIBLE_KEY, WORKSPACE_WIDTH_KEY,
    local_storage, store_bool_key, store_f64_key, store_side_panel_view,
};
use crate::i18n::{self, Locale};
use crate::session_ops::{SessionContextAnchor, estimate_context_chars_for_active_session};
use crate::storage::{ChatSession, ensure_at_least_one, load_sessions, save_sessions};

use super::status_tasks_state::StatusTasksSignals;

/// 供全局 **`Escape`** 处理器按固定顺序关闭的模态/抽屉句柄。
#[derive(Clone, Copy)]
pub struct ShellEscapeSignals {
    pub session_context_menu: RwSignal<Option<SessionContextAnchor>>,
    pub sidebar_rail_ctx_menu: RwSignal<Option<(f64, f64)>>,
    pub chat_find_panel_open: RwSignal<bool>,
    pub sidebar_search_panel_open: RwSignal<bool>,
    pub view_menu_open: RwSignal<bool>,
    pub mobile_nav_open: RwSignal<bool>,
    pub changelist_modal_open: RwSignal<bool>,
    pub settings_modal: RwSignal<bool>,
    pub session_modal: RwSignal<bool>,
}

/// 首次渲染时从 `localStorage` 加载会话列表并设活动会话与草稿。
pub fn wire_initial_sessions_from_storage(
    initialized: RwSignal<bool>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    locale: RwSignal<Locale>,
) {
    Effect::new(move |_| {
        if initialized.get() {
            return;
        }
        let (list, aid) = load_sessions();
        let (list, def_id) = ensure_at_least_one(
            list,
            i18n::default_session_title(locale.get_untracked()).to_string(),
        );
        let pick = aid
            .filter(|id| list.iter().any(|s| s.id == *id))
            .unwrap_or(def_id);
        let d = list
            .iter()
            .find(|s| s.id == pick)
            .map(|s| s.draft.clone())
            .unwrap_or_default();
        sessions.set(list);
        active_id.set(pick);
        draft.set(d);
        initialized.set(true);
    });
}

/// 初始化完成后拉取一次 **`GET /web-ui`**，同步 Markdown / 助手过滤开关。
pub fn wire_web_ui_config_once_after_init(
    initialized: RwSignal<bool>,
    web_ui_config_loaded: RwSignal<bool>,
    markdown_render: RwSignal<bool>,
    apply_assistant_display_filters: RwSignal<bool>,
    locale: RwSignal<Locale>,
) {
    Effect::new({
        move |_| {
            if !initialized.get() || web_ui_config_loaded.get() {
                return;
            }
            web_ui_config_loaded.set(true);
            let locale_val = locale.get_untracked();
            spawn_local(async move {
                if let Ok(c) = fetch_web_ui_config(locale_val).await {
                    markdown_render.set(c.markdown_render);
                    apply_assistant_display_filters.set(c.apply_assistant_display_filters);
                }
            });
        }
    });
}

/// 会话或活动 id 变化时写回 `localStorage`。
pub fn wire_persist_chat_sessions(
    initialized: RwSignal<bool>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
) {
    Effect::new(move |_| {
        if !initialized.get() {
            return;
        }
        let list = sessions.get();
        let aid = active_id.get();
        if aid.is_empty() {
            return;
        }
        save_sessions(&list, Some(&aid));
    });
}

/// 估算当前会话上下文字符数（对照底栏与 **`GET /status`**）。
pub fn wire_context_used_estimate(
    initialized: RwSignal<bool>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    context_used_estimate: RwSignal<usize>,
) {
    Effect::new({
        let sessions = sessions;
        let active_id = active_id;
        let draft = draft;
        move |_| {
            if !initialized.get() {
                return;
            }
            let n = estimate_context_chars_for_active_session(
                &sessions.get(),
                active_id.get().as_str(),
                draft.get().as_str(),
            );
            context_used_estimate.set(n);
        }
    });
}

pub fn wire_persist_side_panel_view_flags(side_panel_view: RwSignal<SidePanelView>) {
    Effect::new(move |_| {
        let v = side_panel_view.get();
        store_side_panel_view(v);
        store_bool_key(WORKSPACE_VISIBLE_KEY, matches!(v, SidePanelView::Workspace));
        store_bool_key(TASKS_VISIBLE_KEY, matches!(v, SidePanelView::Tasks));
    });
}

pub fn wire_persist_status_bar_visible(status_bar_visible: RwSignal<bool>) {
    Effect::new(move |_| {
        store_bool_key(STATUS_BAR_VISIBLE_KEY, status_bar_visible.get());
    });
}

pub fn wire_persist_agent_role(selected_agent_role: RwSignal<Option<String>>) {
    Effect::new(move |_| {
        if let Some(st) = local_storage() {
            match selected_agent_role
                .get()
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(role) => {
                    let _ = st.set_item(AGENT_ROLE_KEY, role);
                }
                None => {
                    let _ = st.remove_item(AGENT_ROLE_KEY);
                }
            }
        }
    });
}

pub fn wire_persist_side_width(side_width: RwSignal<f64>) {
    Effect::new(move |_| {
        store_f64_key(WORKSPACE_WIDTH_KEY, side_width.get());
    });
}

/// 桌面端左侧会话栏收起状态写入 `localStorage`。
pub fn wire_persist_sidebar_rail_collapsed(sidebar_rail_collapsed: RwSignal<bool>) {
    Effect::new(move |_| {
        store_bool_key(SIDEBAR_RAIL_COLLAPSED_KEY, sidebar_rail_collapsed.get());
    });
}

/// 新待审批会话 id 变化时收起审批条展开态。
pub fn wire_approval_expanded_follows_pending(
    pending_approval: RwSignal<Option<(String, String, String)>>,
    last_approval_sid: RwSignal<String>,
    approval_expanded: RwSignal<bool>,
) {
    Effect::new(move |_| {
        if let Some((sid, _, _)) = pending_approval.get() {
            if last_approval_sid.get_untracked() != sid {
                last_approval_sid.set(sid);
                approval_expanded.set(false);
            }
        } else {
            last_approval_sid.set(String::new());
        }
    });
}

pub fn wire_sync_theme_to_storage_and_dom(theme: RwSignal<String>) {
    Effect::new(move |_| {
        let t = theme.get();
        if let Some(st) = local_storage() {
            let _ = st.set_item(THEME_KEY, &t);
        }
        if let Some(doc) = web_sys::window().and_then(|w| w.document())
            && let Some(root) = doc.document_element()
        {
            let _ = root.set_attribute("data-theme", &t);
        }
    });
}

pub fn wire_sync_locale_html_lang(locale: RwSignal<Locale>) {
    Effect::new(move |_| {
        let lang = locale.get().html_lang();
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            let _ = doc
                .document_element()
                .map(|root| root.set_attribute("lang", lang));
        }
    });
}

pub fn wire_sync_bg_decor_to_storage_and_dom(bg_decor: RwSignal<bool>) {
    Effect::new(move |_| {
        store_bool_key(BG_DECOR_KEY, bg_decor.get());
        if let Some(doc) = web_sys::window().and_then(|w| w.document())
            && let Some(root) = doc.document_element()
        {
            if bg_decor.get() {
                let _ = root.remove_attribute("data-bg-decor");
            } else {
                let _ = root.set_attribute("data-bg-decor", "plain");
            }
        }
    });
}

/// 打开设置页面时，用 **`localStorage`** 与 **`/status`** 快照填充 LLM 草稿区。
#[allow(clippy::too_many_arguments)]
pub fn wire_settings_modal_llm_drafts_on_open(
    settings_page: RwSignal<bool>,
    status_tasks: StatusTasksSignals,
    llm_api_base_draft: RwSignal<String>,
    llm_api_base_preset_select: RwSignal<String>,
    llm_model_draft: RwSignal<String>,
    llm_api_key_draft: RwSignal<String>,
    llm_has_saved_key: RwSignal<bool>,
    llm_settings_feedback: RwSignal<Option<String>>,
    executor_llm_api_base_draft: RwSignal<String>,
    executor_llm_api_base_preset_select: RwSignal<String>,
    executor_llm_model_draft: RwSignal<String>,
    executor_llm_api_key_draft: RwSignal<String>,
    executor_llm_has_saved_key: RwSignal<bool>,
    executor_llm_settings_feedback: RwSignal<Option<String>>,
) {
    Effect::new(move |_| {
        if !settings_page.get() {
            return;
        }
        let (stored_base, stored_model) = load_client_llm_text_fields_from_storage();
        let sd = status_tasks.status_data.get_untracked();
        let base = if stored_base.trim().is_empty() {
            sd.as_ref().map(|d| d.api_base.clone()).unwrap_or_default()
        } else {
            stored_base
        };
        let model = if stored_model.trim().is_empty() {
            sd.as_ref().map(|d| d.model.clone()).unwrap_or_default()
        } else {
            stored_model
        };
        llm_api_base_draft.set(base.clone());
        llm_api_base_preset_select.set(
            crate::client_llm_presets::api_base_select_value_for_draft(base.as_str()).to_string(),
        );
        llm_model_draft.set(model);
        llm_api_key_draft.set(String::new());
        llm_has_saved_key.set(client_llm_storage_has_api_key());
        llm_settings_feedback.set(None);

        let (executor_stored_base, executor_stored_model) =
            crate::api::load_executor_llm_text_fields_from_storage();
        let executor_base = if executor_stored_base.trim().is_empty() {
            sd.as_ref()
                .map(|d| d.executor_api_base.clone())
                .unwrap_or_default()
        } else {
            executor_stored_base
        };
        let executor_model = if executor_stored_model.trim().is_empty() {
            sd.as_ref()
                .map(|d| d.executor_model.clone())
                .unwrap_or_default()
        } else {
            executor_stored_model
        };
        executor_llm_api_base_draft.set(executor_base.clone());
        executor_llm_api_base_preset_select.set(
            crate::client_llm_presets::api_base_select_value_for_draft(executor_base.as_str())
                .to_string(),
        );
        executor_llm_model_draft.set(executor_model);
        executor_llm_api_key_draft.set(String::new());
        executor_llm_has_saved_key.set(crate::api::executor_llm_storage_has_api_key());
        executor_llm_settings_feedback.set(None);
    });
}

/// 在输入控件外按 **`Escape`** 按层关闭：会话菜单 → 侧栏菜单 → 查找 → … → 会话管理模态。
pub fn wire_escape_key_layered_dismiss(shell: ShellEscapeSignals) {
    Effect::new(move |_| {
        let h = window_event_listener(leptos::ev::keydown, move |ev: web_sys::KeyboardEvent| {
            if ev.key() != "Escape" {
                return;
            }
            if let Some(t) = ev.target() {
                if let Ok(he) = t.dyn_into::<web_sys::HtmlElement>() {
                    let tag = he.tag_name();
                    if tag.eq_ignore_ascii_case("TEXTAREA")
                        || tag.eq_ignore_ascii_case("INPUT")
                        || tag.eq_ignore_ascii_case("SELECT")
                        || tag.eq_ignore_ascii_case("OPTION")
                    {
                        return;
                    }
                    if he.is_content_editable() {
                        return;
                    }
                }
            }
            ev.prevent_default();
            if shell.session_context_menu.get_untracked().is_some() {
                shell.session_context_menu.set(None);
                return;
            }
            if shell.sidebar_rail_ctx_menu.get_untracked().is_some() {
                shell.sidebar_rail_ctx_menu.set(None);
                return;
            }
            if shell.chat_find_panel_open.get_untracked() {
                shell.chat_find_panel_open.set(false);
                return;
            }
            if shell.sidebar_search_panel_open.get_untracked() {
                shell.sidebar_search_panel_open.set(false);
                return;
            }
            if shell.view_menu_open.get_untracked() {
                shell.view_menu_open.set(false);
                return;
            }
            if shell.mobile_nav_open.get_untracked() {
                shell.mobile_nav_open.set(false);
                return;
            }
            if shell.changelist_modal_open.get_untracked() {
                shell.changelist_modal_open.set(false);
                return;
            }
            if shell.settings_modal.get_untracked() {
                shell.settings_modal.set(false);
                return;
            }
            if shell.session_modal.get_untracked() {
                shell.session_modal.set(false);
            }
        });
        on_cleanup(move || h.remove());
    });
}
