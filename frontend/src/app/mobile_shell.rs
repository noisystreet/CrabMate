//! 窄屏壳层：底部 Tab、侧栏 Sheet 遮罩与 Tab/侧栏视图同步。

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::app_prefs::{MobileShellTab, SidePanelView};
use crate::i18n::{self, Locale};

/// 选择底部 Tab 并同步右侧面板视图（窄屏主路径）。
pub fn select_mobile_shell_tab(
    tab: MobileShellTab,
    mobile_shell_tab: RwSignal<MobileShellTab>,
    side_panel_view: RwSignal<SidePanelView>,
    mobile_nav_open: RwSignal<bool>,
) {
    mobile_shell_tab.set(tab);
    side_panel_view.set(tab.to_side_panel());
    mobile_nav_open.set(false);
}

pub struct WireMobileShellSyncSignals {
    pub is_narrow_viewport: RwSignal<bool>,
    pub mobile_shell_tab: RwSignal<MobileShellTab>,
    pub side_panel_view: RwSignal<SidePanelView>,
    pub mobile_nav_open: RwSignal<bool>,
    pub editor_layout_mode: RwSignal<bool>,
}

/// 窄屏下侧栏关闭时回到 Chat Tab；打开侧栏时关闭左侧抽屉（不自动把侧栏视图同步到底部 Tab，避免默认 Workspace 遮住聊天列）。
pub fn wire_mobile_shell_tab_sync(sig: WireMobileShellSyncSignals) {
    let is_narrow_viewport = sig.is_narrow_viewport;
    let mobile_shell_tab = sig.mobile_shell_tab;
    let side_panel_view = sig.side_panel_view;
    let mobile_nav_open = sig.mobile_nav_open;

    Effect::new(move |_| {
        if !is_narrow_viewport.get() {
            return;
        }
        let view = side_panel_view.get();
        if matches!(view, SidePanelView::None)
            && mobile_shell_tab.get_untracked() != MobileShellTab::Chat
        {
            mobile_shell_tab.set(MobileShellTab::Chat);
        }
        if !matches!(view, SidePanelView::None) {
            mobile_nav_open.set(false);
        }
    });
}

/// `data-mobile-tab` 与 Sheet 打开时的 `body` 滚动锁定。
pub fn wire_mobile_shell_dom_and_scroll_lock(sig: WireMobileShellSyncSignals) {
    let is_narrow_viewport = sig.is_narrow_viewport;
    let mobile_shell_tab = sig.mobile_shell_tab;
    let side_panel_view = sig.side_panel_view;
    let editor_layout_mode = sig.editor_layout_mode;

    Effect::new(move |_| {
        let narrow = is_narrow_viewport.get();
        let tab = mobile_shell_tab.get();
        let view = side_panel_view.get();
        let ide = editor_layout_mode.get();

        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            if let Some(shell_main) = doc.get_element_by_id("layout-mode-panel-main") {
                if narrow && !ide {
                    let _ = shell_main.set_attribute("data-mobile-tab", tab.dom_slug());
                } else {
                    let _ = shell_main.remove_attribute("data-mobile-tab");
                }
            }
            if let Some(body) = doc.body() {
                let sheet_overlay = narrow
                    && !ide
                    && matches!(tab, MobileShellTab::Chat)
                    && !matches!(view, SidePanelView::None);
                let style = body.unchecked_ref::<web_sys::HtmlElement>().style();
                if sheet_overlay {
                    let _ = style.set_property("overflow", "hidden");
                } else {
                    let _ = style.remove_property("overflow");
                }
            }
        }
    });
}

#[derive(Clone, Copy)]
pub struct MobileBottomTabBarSignals {
    pub locale: RwSignal<Locale>,
    pub is_narrow_viewport: RwSignal<bool>,
    pub mobile_shell_tab: RwSignal<MobileShellTab>,
    pub side_panel_view: RwSignal<SidePanelView>,
    pub mobile_nav_open: RwSignal<bool>,
    pub editor_layout_mode: RwSignal<bool>,
}

#[component]
fn MobileBottomTabButton(
    locale: RwSignal<Locale>,
    tab: MobileShellTab,
    active_tab: RwSignal<MobileShellTab>,
    label_fn: fn(Locale) -> &'static str,
    test_id: &'static str,
    mobile_shell_tab: RwSignal<MobileShellTab>,
    side_panel_view: RwSignal<SidePanelView>,
    mobile_nav_open: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <button
            type="button"
            role="tab"
            class="mobile-bottom-tab"
            class:active=move || active_tab.get() == tab
            aria-selected=move || active_tab.get() == tab
            data-testid=test_id
            on:click=move |_| {
                select_mobile_shell_tab(tab, mobile_shell_tab, side_panel_view, mobile_nav_open);
            }
        >
            {move || label_fn(locale.get())}
        </button>
    }
}

#[component]
pub fn MobileBottomTabBar(signals: MobileBottomTabBarSignals) -> impl IntoView {
    let MobileBottomTabBarSignals {
        locale,
        is_narrow_viewport,
        mobile_shell_tab,
        side_panel_view,
        mobile_nav_open,
        editor_layout_mode,
    } = signals;
    view! {
        <Show when=move || is_narrow_viewport.get() && !editor_layout_mode.get()>
            <nav
                class="mobile-bottom-tab-bar"
                role="tablist"
                data-testid="mobile-bottom-tab-bar"
                prop:aria-label=move || i18n::mobile_tab_bar_aria(locale.get())
            >
                <MobileBottomTabButton
                    locale=locale
                    tab=MobileShellTab::Chat
                    active_tab=mobile_shell_tab
                    label_fn=i18n::mobile_tab_chat
                    test_id="mobile-tab-chat"
                    mobile_shell_tab=mobile_shell_tab
                    side_panel_view=side_panel_view
                    mobile_nav_open=mobile_nav_open
                />
                <MobileBottomTabButton
                    locale=locale
                    tab=MobileShellTab::Workspace
                    active_tab=mobile_shell_tab
                    label_fn=i18n::mobile_tab_workspace
                    test_id="mobile-tab-workspace"
                    mobile_shell_tab=mobile_shell_tab
                    side_panel_view=side_panel_view
                    mobile_nav_open=mobile_nav_open
                />
                <MobileBottomTabButton
                    locale=locale
                    tab=MobileShellTab::Tasks
                    active_tab=mobile_shell_tab
                    label_fn=i18n::mobile_tab_tasks
                    test_id="mobile-tab-tasks"
                    mobile_shell_tab=mobile_shell_tab
                    side_panel_view=side_panel_view
                    mobile_nav_open=mobile_nav_open
                />
                <MobileBottomTabButton
                    locale=locale
                    tab=MobileShellTab::More
                    active_tab=mobile_shell_tab
                    label_fn=i18n::mobile_tab_more
                    test_id="mobile-tab-more"
                    mobile_shell_tab=mobile_shell_tab
                    side_panel_view=side_panel_view
                    mobile_nav_open=mobile_nav_open
                />
            </nav>
        </Show>
    }
}

#[derive(Clone, Copy)]
pub struct MobileSideSheetBackdropSignals {
    pub is_narrow_viewport: RwSignal<bool>,
    pub mobile_shell_tab: RwSignal<MobileShellTab>,
    pub side_panel_view: RwSignal<SidePanelView>,
    pub editor_layout_mode: RwSignal<bool>,
}

#[component]
pub fn MobileSideSheetBackdrop(signals: MobileSideSheetBackdropSignals) -> impl IntoView {
    let MobileSideSheetBackdropSignals {
        is_narrow_viewport,
        mobile_shell_tab,
        side_panel_view,
        editor_layout_mode,
    } = signals;
    view! {
        <Show when=move || {
            is_narrow_viewport.get()
                && !editor_layout_mode.get()
                && matches!(mobile_shell_tab.get(), MobileShellTab::Chat)
                && !matches!(side_panel_view.get(), SidePanelView::None)
        }>
            <button
                type="button"
                class="side-panel-sheet-backdrop"
                data-testid="side-panel-sheet-backdrop"
                aria-label="Close"
                on:click=move |_| side_panel_view.set(SidePanelView::None)
            />
        </Show>
    }
}
