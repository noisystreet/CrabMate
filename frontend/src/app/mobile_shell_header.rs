//! 统一壳顶栏：会话与 IDE 共用 leading / center / trailing 三槽；
//! 菜单下拉与遮罩走 [`ShellTopbarFileMenu`] / [`ShellTopbarMenuBackdrop`]。

use leptos::prelude::*;

use crate::i18n::{self, Locale};
use crate::mobile_remote::mobile_remote_client;
use crate::tauri_shell::tauri_shell_available;

use super::app_shell_ctx::MobileShellHeaderSignals;
use super::ide_menu_bar::{
    IdeMenuBarBridge, IdeMenuBarTopbarContent, IdeMenuId, ShellTopbarFileMenu,
    ShellTopbarMenuBackdrop,
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
            <div class="shell-topbar-start shell-topbar-menus">
                <ShellTopbarFileMenu
                    locale=locale
                    workspace_pick=workspace_pick
                    open_menu=open_menu
                    menubar_dropdown_open=menubar_dropdown_open
                />
            </div>
            <ShellTopbarMenuBackdrop
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
fn ShellTopbarFileStatusSlot(
    editor_layout_mode: RwSignal<bool>,
    ide_menu_bar_bridge: RwSignal<Option<IdeMenuBarBridge>>,
) -> impl IntoView {
    view! {
        <div
            class="shell-topbar-file-status"
            class:shell-topbar-file-status--active=move || editor_layout_mode.get()
            data-testid="shell-topbar-file-status"
        >
            <Show when=move || editor_layout_mode.get()>
                {move || match ide_menu_bar_bridge.get() {
                    Some(bridge) => {
                        let ide_path = bridge.signals.ide_path;
                        let ide_text = bridge.signals.ide_text;
                        let ide_baseline = bridge.signals.ide_baseline;
                        view! {
                            <Show when=move || ide_text.get() != ide_baseline.get()>
                                <span class="ide-dirty-dot" aria-hidden="true">"●"</span>
                            </Show>
                            <span class="shell-topbar-file-path">
                                {move || ide_path.get().unwrap_or_default()}
                            </span>
                        }
                        .into_any()
                    }
                    None => ().into_any(),
                }}
            </Show>
        </div>
    }
}

pub fn mobile_shell_header_view(signals: MobileShellHeaderSignals) -> impl IntoView {
    let MobileShellHeaderSignals {
        locale,
        editor_layout_mode,
        is_narrow_viewport,
        ide_menu_bar_bridge,
        layout_toggle,
        workspace_pick,
        ide_menubar_dropdown_open,
    } = signals;
    let show_layout_toggle = move || !is_narrow_viewport.get() && !mobile_remote_client();
    view! {
        <header
            class="shell-main-header-mobile shell-topbar"
            class:ide-menu-bar=move || editor_layout_mode.get()
            role=move || shell_topbar_a11y(editor_layout_mode.get(), locale.get()).0
            data-testid=move || shell_topbar_a11y(editor_layout_mode.get(), locale.get()).1
            prop:aria-label=move || shell_topbar_a11y(editor_layout_mode.get(), locale.get()).2
        >
            <div class="shell-topbar-leading">
                <Show when=show_layout_toggle>
                    <div class="shell-topbar-start shell-topbar-layout-start">
                        <LayoutModeSegment
                            locale=locale
                            layout_toggle=layout_toggle
                            extra_class="shell-topbar-layout-toggle"
                        />
                    </div>
                </Show>
                <Show
                    when=move || editor_layout_mode.get()
                    fallback=move || {
                        view! {
                            <ShellTopbarChatMenus
                                locale=locale
                                workspace_pick=workspace_pick
                                menubar_dropdown_open=ide_menubar_dropdown_open
                            />
                        }
                    }
                >
                    <ShellTopbarIdeMenus ide_menu_bar_bridge=ide_menu_bar_bridge />
                </Show>
            </div>
            <ShellTopbarWorkspaceRoot pick=workspace_pick />
            <div class="shell-topbar-trailing">
                <ShellTopbarFileStatusSlot
                    editor_layout_mode=editor_layout_mode
                    ide_menu_bar_bridge=ide_menu_bar_bridge
                />
                <div class="shell-topbar-end">
                    <Show when=move || {
                        // 桌面无边框壳才要最小化/最大化/关闭；Android 远程壳不显示
                        tauri_shell_available() && !mobile_remote_client()
                    }>
                        <TauriWindowControls locale=locale />
                    </Show>
                </div>
            </div>
        </header>
    }
}
