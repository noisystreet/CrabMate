//! 统一壳顶栏：会话模式（☰ + 文件菜单）与 IDE 模式（文件 / 编辑 / 视图）共用同一 DOM；
//! 工作区根路径固定于顶栏正中；对话 / 编辑器切换控件固定于最左侧。

use leptos::prelude::*;

use crate::i18n::{self, Locale};
use crate::tauri_shell::tauri_shell_available;

use super::app_shell_ctx::MobileShellHeaderSignals;
use super::ide_menu_bar::{
    ChatShellFileMenu, IdeMenuBarBridge, IdeMenuBarTopbarContent, IdeMenuId,
};
use super::layout_mode_segment::LayoutModeSegment;
use super::tauri_window_controls::TauriWindowControls;
use super::workspace_root_actions::{ShellTopbarWorkspaceRoot, WorkspaceRootPickHandle};

fn shell_topbar_a11y(ide: bool, locale: Locale) -> (&'static str, &'static str, &'static str) {
    if ide {
        ("menubar", "ide-menu-bar", i18n::ide_menu_bar_aria(locale))
    } else {
        (
            "banner",
            "shell-main-header-mobile",
            i18n::app_shell_title(locale),
        )
    }
}

#[component]
fn ShellTopbarChatMenus(
    locale: RwSignal<Locale>,
    mobile_nav_open: RwSignal<bool>,
    workspace_pick: WorkspaceRootPickHandle,
    menubar_dropdown_open: RwSignal<bool>,
) -> impl IntoView {
    let open_menu = RwSignal::new(None::<IdeMenuId>);
    Effect::new(move |_| {
        if !menubar_dropdown_open.get() {
            open_menu.set(None);
        }
    });
    view! {
        <>
            <div class="shell-topbar-start shell-topbar-nav">
                <button
                    type="button"
                    class="btn btn-icon"
                    prop:aria-label=move || i18n::mobile_open_menu(locale.get())
                    on:click=move |_| mobile_nav_open.update(|o| *o = !*o)
                >
                    "☰"
                </button>
            </div>
            <ChatShellFileMenu
                locale=locale
                workspace_pick=workspace_pick
                open_menu=open_menu
                menubar_dropdown_open=menubar_dropdown_open
            />
        </>
    }
}

#[component]
fn ShellTopbarIdeMenus(ide_menu_bar_bridge: RwSignal<Option<IdeMenuBarBridge>>) -> impl IntoView {
    move || match ide_menu_bar_bridge.get() {
        Some(bridge) => view! { <IdeMenuBarTopbarContent bridge=bridge /> }.into_any(),
        None => ().into_any(),
    }
}

#[component]
fn ShellTopbarIdeFileStatus(
    ide_menu_bar_bridge: RwSignal<Option<IdeMenuBarBridge>>,
) -> impl IntoView {
    move || match ide_menu_bar_bridge.get() {
        Some(bridge) => {
            let ide_path = bridge.signals.ide_path;
            let ide_text = bridge.signals.ide_text;
            let ide_baseline = bridge.signals.ide_baseline;
            view! {
                <div class="shell-topbar-file-status" data-testid="shell-topbar-file-status">
                    <Show when=move || ide_text.get() != ide_baseline.get()>
                        <span class="ide-dirty-dot" aria-hidden="true">"●"</span>
                    </Show>
                    <span class="ide-menu-bar-path">{move || ide_path.get().unwrap_or_default()}</span>
                </div>
            }
            .into_any()
        }
        None => ().into_any(),
    }
}

pub fn mobile_shell_header_view(signals: MobileShellHeaderSignals) -> impl IntoView {
    let MobileShellHeaderSignals {
        mobile_nav_open,
        locale,
        editor_layout_mode,
        ide_menu_bar_bridge,
        layout_toggle,
        workspace_pick,
        ide_menubar_dropdown_open,
    } = signals;
    view! {
        <header
            class="shell-main-header-mobile shell-topbar"
            class:shell-topbar--app=move || tauri_shell_available()
            class:ide-menu-bar=move || editor_layout_mode.get()
            role=move || shell_topbar_a11y(editor_layout_mode.get(), locale.get()).0
            data-testid=move || shell_topbar_a11y(editor_layout_mode.get(), locale.get()).1
            prop:aria-label=move || shell_topbar_a11y(editor_layout_mode.get(), locale.get()).2
        >
            <div class="shell-topbar-start shell-topbar-layout-start">
                <LayoutModeSegment
                    locale=locale
                    layout_toggle=layout_toggle
                    extra_class="shell-topbar-layout-toggle"
                />
            </div>
            <Show
                when=move || editor_layout_mode.get()
                fallback=move || {
                    view! {
                        <ShellTopbarChatMenus
                            locale=locale
                            mobile_nav_open=mobile_nav_open
                            workspace_pick=workspace_pick
                            menubar_dropdown_open=ide_menubar_dropdown_open
                        />
                    }
                }
            >
                <ShellTopbarIdeMenus ide_menu_bar_bridge=ide_menu_bar_bridge />
            </Show>
            <ShellTopbarWorkspaceRoot pick=workspace_pick />
            <Show when=move || editor_layout_mode.get()>
                <ShellTopbarIdeFileStatus ide_menu_bar_bridge=ide_menu_bar_bridge />
            </Show>
            <div class="shell-topbar-end">
                <Show when=move || tauri_shell_available()>
                    <TauriWindowControls locale=locale />
                </Show>
            </div>
        </header>
    }
}
