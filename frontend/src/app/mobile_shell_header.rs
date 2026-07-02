//! 统一壳顶栏：会话模式（☰ + 标题）与 IDE 模式（文件 / 编辑 / 视图 + 路径）共用同一 DOM；
//! 对话 / 编辑器切换控件固定于顶栏最左侧，避免侧栏与 IDE 左栏各一份导致位置不一致。

use leptos::prelude::*;

use crate::i18n::{self, Locale};
use crate::tauri_shell::tauri_shell_available;

use super::app_shell_ctx::MobileShellHeaderSignals;
use super::ide_menu_bar::{IdeMenuBarBridge, IdeMenuBarTopbarContent};
use super::layout_mode_segment::LayoutModeSegment;
use super::tauri_window_controls::TauriWindowControls;

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
fn ShellTopbarChatBody(locale: RwSignal<Locale>, mobile_nav_open: RwSignal<bool>) -> impl IntoView {
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
            <span class="shell-topbar-title shell-main-header-title">
                {move || i18n::app_shell_title(locale.get())}
            </span>
        </>
    }
}

#[component]
fn ShellTopbarIdeBody(ide_menu_bar_bridge: RwSignal<Option<IdeMenuBarBridge>>) -> impl IntoView {
    move || match ide_menu_bar_bridge.get() {
        Some(bridge) => view! { <IdeMenuBarTopbarContent bridge=bridge /> }.into_any(),
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
                        <ShellTopbarChatBody locale=locale mobile_nav_open=mobile_nav_open />
                    }
                }
            >
                <ShellTopbarIdeBody ide_menu_bar_bridge=ide_menu_bar_bridge />
            </Show>
            <div class="shell-topbar-end">
                <Show when=move || tauri_shell_available()>
                    <TauriWindowControls locale=locale />
                </Show>
            </div>
        </header>
    }
}
