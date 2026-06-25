//! 壳层偏好经 **`/user-data/prefs`** 读写（不再使用 `localStorage`）。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::user_data::{UserPrefsDto, fetch_user_data_prefs, put_user_data_prefs};
use crate::app::app_signals::AppSignals;
use crate::app_prefs::SidePanelView;
use crate::i18n::Locale;
const PERSIST_DEBOUNCE_MS: u32 = 400;

fn side_panel_from_slug(s: &str) -> SidePanelView {
    match s.trim() {
        "none" => SidePanelView::None,
        "tasks" => SidePanelView::Tasks,
        "debug" => SidePanelView::DebugConsole,
        _ => SidePanelView::Workspace,
    }
}

fn side_panel_slug(v: SidePanelView) -> &'static str {
    match v {
        SidePanelView::None => "none",
        SidePanelView::Workspace => "workspace",
        SidePanelView::Tasks => "tasks",
        SidePanelView::DebugConsole => "debug",
    }
}

/// IDE 模式下侧栏由 CSS 隐藏，勿把 `collapsed=true` 写入 prefs（旧版进入 IDE 时会误设为 true）。
fn effective_sidebar_rail_collapsed_for_persist(app: &AppSignals) -> bool {
    if app.shell_ui.editor_layout_mode.get_untracked() {
        return false;
    }
    app.sidebar.sidebar_rail_collapsed.get_untracked()
}

pub fn build_prefs_dto(app: &AppSignals) -> UserPrefsDto {
    UserPrefsDto {
        locale: Some(
            app.shell_ui
                .locale
                .get_untracked()
                .storage_slug()
                .to_string(),
        ),
        theme: Some(app.shell_ui.theme.get_untracked()),
        side_panel_view: Some(
            side_panel_slug(app.shell_ui.side_panel_view.get_untracked()).to_string(),
        ),
        side_width: Some(app.shell_ui.side_width.get_untracked()),
        editor_layout_mode: Some(app.shell_ui.editor_layout_mode.get_untracked()),
        timeline_panel_expanded: Some(app.chat_composer.timeline_panel_expanded.get_untracked()),
        sidebar_rail_collapsed: Some(effective_sidebar_rail_collapsed_for_persist(app)),
        session_ui_font: Some(app.shell_ui.session_ui_font.get_untracked()),
        session_chat_font: Some(app.shell_ui.session_chat_font.get_untracked()),
        ide_editor_font: Some(app.ide_editor.font_slug.get_untracked()),
        ide_editor_font_size: Some(app.ide_editor.font_size_px.get_untracked().round() as u32),
        ide_editor_line_numbers: Some(app.ide_editor.line_numbers.get_untracked()),
        ide_editor_word_wrap: Some(app.ide_editor.word_wrap.get_untracked()),
        ide_editor_tab_size: Some(app.ide_editor.tab_size.get_untracked() as u32),
        bg_decor: Some(app.shell_ui.bg_decor.get_untracked()),
        status_bar_visible: Some(app.shell_ui.status_bar_visible.get_untracked()),
        cm_role: app
            .llm_settings
            .selected_agent_role
            .get_untracked()
            .filter(|s| !s.trim().is_empty()),
        disable_readonly_tool_ttl_cache: Some(
            !crate::api::client_llm_storage::load_readonly_tool_ttl_cache_follow_server_from_memory(
            ),
        ),
        last_workspace_root: None,
    }
}

fn apply_shell_prefs_dto(app: &AppSignals, dto: &UserPrefsDto) {
    if let Some(ref t) = dto.theme {
        app.shell_ui
            .theme
            .set(crate::app_prefs::normalize_theme_slug(t));
    }
    if let Some(b) = dto.bg_decor {
        app.shell_ui.bg_decor.set(b);
    }
    if let Some(ref loc) = dto.locale {
        app.shell_ui.locale.set(Locale::from_storage_slug(loc));
    }
    if let Some(v) = dto.status_bar_visible {
        app.shell_ui.status_bar_visible.set(v);
    }
    if let Some(ref sp) = dto.side_panel_view {
        app.shell_ui.side_panel_view.set(side_panel_from_slug(sp));
    }
    if let Some(w) = dto.side_width {
        app.shell_ui.side_width.set(w);
    }
    if let Some(m) = dto.editor_layout_mode {
        app.shell_ui.editor_layout_mode.set(m);
    }
    if let Some(t) = dto.timeline_panel_expanded {
        app.chat_composer.timeline_panel_expanded.set(t);
    }
    if let Some(c) = dto.sidebar_rail_collapsed {
        let in_editor = dto.editor_layout_mode.unwrap_or(false);
        app.sidebar
            .sidebar_rail_collapsed
            .set(if in_editor && c { false } else { c });
    }
    if let Some(ref f) = dto.session_ui_font {
        app.shell_ui.session_ui_font.set(
            crate::session_typography_prefs::normalize_session_ui_font(f),
        );
    }
    if let Some(ref f) = dto.session_chat_font {
        app.shell_ui
            .session_chat_font
            .set(crate::session_typography_prefs::normalize_session_chat_font(f));
    }
}

fn apply_ide_and_llm_prefs_dto(app: &AppSignals, dto: &UserPrefsDto) {
    if let Some(ref f) = dto.ide_editor_font {
        app.ide_editor
            .font_slug
            .set(crate::ide_editor_prefs::normalize_font_slug(f));
    }
    if let Some(n) = dto.ide_editor_font_size {
        app.ide_editor
            .font_size_px
            .set((n as f64).clamp(10.0, 28.0));
    }
    if let Some(ln) = dto.ide_editor_line_numbers {
        app.ide_editor.line_numbers.set(ln);
    }
    if let Some(ww) = dto.ide_editor_word_wrap {
        app.ide_editor.word_wrap.set(ww);
    }
    if let Some(ts) = dto.ide_editor_tab_size {
        app.ide_editor.tab_size.set(ts.clamp(2, 8) as u8);
    }
    if let Some(ref r) = dto.cm_role {
        let t = r.trim();
        app.llm_settings.selected_agent_role.set(if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        });
    }
    if let Some(d) = dto.disable_readonly_tool_ttl_cache {
        crate::api::client_llm_storage::set_readonly_tool_ttl_cache_follow_server_in_memory(!d);
    }
}

fn apply_prefs_dto(app: &AppSignals, dto: &UserPrefsDto) {
    apply_shell_prefs_dto(app, dto);
    apply_ide_and_llm_prefs_dto(app, dto);
}

/// 首启从服务端加载偏好并写入信号（随后由 DOM sync Effect 反映到页面）。
pub fn wire_load_user_prefs_from_server(app: AppSignals) {
    let loaded = RwSignal::new(false);
    Effect::new(move |_| {
        if loaded.get() {
            return;
        }
        loaded.set(true);
        let app = app.clone();
        spawn_local(async move {
            let loc = app.shell_ui.locale.get_untracked();
            if let Ok(dto) = fetch_user_data_prefs(loc).await {
                apply_prefs_dto(&app, &dto);
                crate::app::shell_prefs_storage::apply_loaded_prefs_to_dom(&app);
            }
        });
    });
}

/// 偏好变更防抖写入 `/user-data/prefs`。
pub fn wire_persist_user_prefs_to_server(app: AppSignals) {
    let debounce_tick = StoredValue::new(Arc::new(AtomicU64::new(0)));

    Effect::new(move |_| {
        let _ = app.shell_ui.theme.get();
        let _ = app.shell_ui.bg_decor.get();
        let _ = app.shell_ui.locale.get();
        let _ = app.shell_ui.status_bar_visible.get();
        let _ = app.shell_ui.side_panel_view.get();
        let _ = app.shell_ui.side_width.get();
        let _ = app.shell_ui.editor_layout_mode.get();
        let _ = app.chat_composer.timeline_panel_expanded.get();
        let _ = app.sidebar.sidebar_rail_collapsed.get();
        let _ = app.shell_ui.session_ui_font.get();
        let _ = app.shell_ui.session_chat_font.get();
        let _ = app.ide_editor.font_slug.get();
        let _ = app.ide_editor.font_size_px.get();
        let _ = app.ide_editor.line_numbers.get();
        let _ = app.ide_editor.word_wrap.get();
        let _ = app.ide_editor.tab_size.get();
        let _ = app.llm_settings.selected_agent_role.get();

        let ctr = debounce_tick.get_value();
        let prev = ctr.fetch_add(1, Ordering::Relaxed);
        let tick = prev.wrapping_add(1);
        let ctr2 = Arc::clone(&ctr);
        let app2 = app.clone();
        spawn_local(async move {
            TimeoutFuture::new(PERSIST_DEBOUNCE_MS).await;
            if ctr2.load(Ordering::Relaxed) != tick {
                return;
            }
            let loc = app2.shell_ui.locale.get_untracked();
            let dto = build_prefs_dto(&app2);
            let _ = put_user_data_prefs(&dto, loc).await;
        });
    });
}
