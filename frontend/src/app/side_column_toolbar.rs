//! 侧栏拖拽手柄与壳层工具栏（从 `side_column.rs` 拆出以降低单组件圈复杂度）。

use leptos::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::app::github_embed_page::{
    github_repo_btn_disabled, try_open_github_embed_from_repo, use_github_embed_signals,
};
use crate::app_prefs::SidePanelView;
use crate::i18n::{self, Locale};
use crate::workspace_shell::begin_side_column_resize;

use super::status_tasks_state::StatusTasksSignals;

/// 侧栏隐藏时工具栏是否以浮动条渲染（须脱出 `side-column-rail-only` 父节点）。
pub(crate) fn side_toolbar_rail_float(view: SidePanelView) -> bool {
    matches!(view, SidePanelView::None)
}

pub(super) type SideResizeHandlesCell = Rc<
    RefCell<
        Option<(
            leptos_dom::helpers::WindowListenerHandle,
            leptos_dom::helpers::WindowListenerHandle,
        )>,
    >,
>;

#[derive(Clone)]
pub(super) struct SideColumnResizeToolbarSignals {
    pub locale: RwSignal<Locale>,
    pub side_resize_dragging: RwSignal<bool>,
    pub side_panel_view: RwSignal<SidePanelView>,
    pub side_width: RwSignal<f64>,
    pub side_resize_session: Rc<RefCell<Option<(f64, f64)>>>,
    pub side_resize_handles: SideResizeHandlesCell,
    pub view_menu_open: RwSignal<bool>,
    pub status_bar_visible: RwSignal<bool>,
    pub settings_page: RwSignal<bool>,
    pub status_tasks: StatusTasksSignals,
}

#[derive(Clone, Copy)]
struct SidePanelViewPickerProps {
    locale: RwSignal<Locale>,
    side_panel_view: RwSignal<SidePanelView>,
    view_menu_open: RwSignal<bool>,
}

#[component]
fn SidePanelViewPickerTrigger(props: SidePanelViewPickerProps) -> impl IntoView {
    let SidePanelViewPickerProps {
        locale,
        side_panel_view,
        view_menu_open,
    } = props;
    view! {
        <button
            type="button"
            class="btn btn-secondary btn-sm toolbar-view-trigger shell-toolbar-icon-btn"
            data-testid="side-view-trigger"
            class:active=move || !matches!(side_panel_view.get(), SidePanelView::None)
            class:toolbar-view-trigger-open=move || view_menu_open.get()
            on:click=move |_| view_menu_open.update(|o| *o = !*o)
            prop:title=move || i18n::side_view_menu_title(locale.get())
            prop:aria-label=move || i18n::side_view_menu_aria(locale.get())
        >
            <span class="toolbar-view-trigger-inner" aria-hidden="true">
                <svg
                    class="shell-toolbar-icon"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <rect x="3" y="3" width="7" height="18" rx="1" ry="1" />
                    <rect x="14" y="3" width="7" height="18" rx="1" ry="1" />
                </svg>
                <svg
                    class="toolbar-view-chevron"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <polyline points="6 9 12 15 18 9" />
                </svg>
            </span>
        </button>
    }
}

#[component]
fn SidePanelViewPickerMenu(props: SidePanelViewPickerProps) -> impl IntoView {
    let SidePanelViewPickerProps {
        locale,
        side_panel_view,
        view_menu_open,
    } = props;
    view! {
        <div class="toolbar-view-menu" role="menu" prop:aria-label=move || i18n::side_view_menu_aria(locale.get())>
            <button
                type="button"
                class="toolbar-view-menu-item"
                class:active=move || matches!(side_panel_view.get(), SidePanelView::None)
                role="menuitem"
                on:click=move |_| {
                    side_panel_view.set(SidePanelView::None);
                    view_menu_open.set(false);
                }
            >
                {move || i18n::side_panel_hide(locale.get())}
            </button>
            <button
                type="button"
                class="toolbar-view-menu-item"
                data-testid="side-panel-workspace-menu"
                class:active=move || matches!(side_panel_view.get(), SidePanelView::Workspace)
                role="menuitem"
                on:click=move |_| {
                    side_panel_view.set(SidePanelView::Workspace);
                    view_menu_open.set(false);
                }
            >
                {move || i18n::side_panel_workspace(locale.get())}
            </button>
            <button
                type="button"
                class="toolbar-view-menu-item"
                class:active=move || matches!(side_panel_view.get(), SidePanelView::Tasks)
                role="menuitem"
                on:click=move |_| {
                    side_panel_view.set(SidePanelView::Tasks);
                    view_menu_open.set(false);
                }
            >
                {move || i18n::side_panel_tasks(locale.get())}
            </button>
            <button
                type="button"
                class="toolbar-view-menu-item"
                class:active=move || matches!(side_panel_view.get(), SidePanelView::DebugConsole)
                role="menuitem"
                prop:title=move || i18n::side_debug_console_title(locale.get())
                on:click=move |_| {
                    side_panel_view.set(SidePanelView::DebugConsole);
                    view_menu_open.set(false);
                }
            >
                {move || i18n::side_debug_console_btn(locale.get())}
            </button>
        </div>
    }
}

#[component]
fn SideColumnResizeDragHandle(
    locale: RwSignal<Locale>,
    side_panel_view: RwSignal<SidePanelView>,
    side_width: RwSignal<f64>,
    side_resize_dragging: RwSignal<bool>,
    side_resize_session: Rc<RefCell<Option<(f64, f64)>>>,
    side_resize_handles: SideResizeHandlesCell,
) -> impl IntoView {
    view! {
        <div
            class="column-resize-handle"
            class:column-resize-handle-off=move || {
                matches!(side_panel_view.get(), SidePanelView::None)
            }
            role="separator"
            aria-orientation="vertical"
            prop:aria-label=move || i18n::side_resize_handle(locale.get())
            on:mousedown={
                let sess = Rc::clone(&side_resize_session);
                let hands = Rc::clone(&side_resize_handles);
                move |ev| {
                    begin_side_column_resize(
                        ev,
                        side_panel_view,
                        side_width,
                        side_resize_dragging,
                        Rc::clone(&sess),
                        Rc::clone(&hands),
                    );
                }
            }
        ></div>
    }
}

#[component]
fn SideToolbarGithubRepoBtn(
    locale: RwSignal<Locale>,
    view_menu_open: RwSignal<bool>,
    status_tasks: StatusTasksSignals,
) -> impl IntoView {
    let embed = use_github_embed_signals();
    view! {
        <button
            type="button"
            class="btn btn-secondary btn-sm shell-toolbar-icon-btn shell-toolbar-github-btn"
            data-testid="side-toolbar-github-repo"
            prop:disabled=move || github_repo_btn_disabled(status_tasks.github_repo.get().as_ref())
            on:click=move |_| {
                view_menu_open.set(false);
                let _ = try_open_github_embed_from_repo(
                    status_tasks.github_repo.get_untracked(),
                    locale.get_untracked(),
                    embed,
                );
            }
            prop:title=move || i18n::side_github_repo_btn_title(locale.get())
            prop:aria-label=move || {
                status_tasks
                    .github_repo
                    .get()
                    .and_then(|r| r.repo)
                    .map(|name| i18n::side_github_repo_btn_aria(locale.get(), &name))
                    .unwrap_or_else(|| i18n::side_github_repo_btn_title(locale.get()).to_string())
            }
        >
            <svg
                class="shell-toolbar-icon"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
            >
                <path d="M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-.1.58-.18 1-.26.18-.04.36-.1.55-.18.06 0 .12-.01.18-.02A4 4 0 0 0 12 2c-1.74 0-3.24.89-4.12 2.24.06.01.12.02.18.02.19.08.37.14.55.18.42.08.72.16 1 .26-.73 1.02-1.08 2.25-1 3.5.01 3.5 3 5.5 6 5.5a4.8 4.8 0 0 0-1 3.5v4" />
            </svg>
        </button>
    }
}

#[component]
fn SideColumnShellToolbarIcons(
    rail_float: bool,
    locale: RwSignal<Locale>,
    side_panel_view: RwSignal<SidePanelView>,
    view_menu_open: RwSignal<bool>,
    status_bar_visible: RwSignal<bool>,
    settings_page: RwSignal<bool>,
    status_tasks: StatusTasksSignals,
) -> impl IntoView {
    view! {
        <div
            class="shell-main-toolbar"
            class:shell-main-toolbar--rail-float=move || rail_float
            data-testid="side-shell-toolbar"
            role="toolbar"
            prop:aria-label=move || i18n::side_toolbar_aria(locale.get())
        >
            <SideToolbarGithubRepoBtn locale=locale view_menu_open=view_menu_open status_tasks=status_tasks />
            <div class="toolbar-view-wrap">
                <Show when=move || view_menu_open.get()>
                    <div
                        class="toolbar-view-backdrop"
                        on:click=move |_| view_menu_open.set(false)
                    ></div>
                </Show>
                <SidePanelViewPickerTrigger props=SidePanelViewPickerProps {
                    locale,
                    side_panel_view,
                    view_menu_open,
                } />
                <Show when=move || view_menu_open.get()>
                    <SidePanelViewPickerMenu props=SidePanelViewPickerProps {
                        locale,
                        side_panel_view,
                        view_menu_open,
                    } />
                </Show>
            </div>
            <button
                type="button"
                class="btn btn-secondary btn-sm shell-toolbar-icon-btn"
                class:active=move || status_bar_visible.get()
                on:click=move |_| status_bar_visible.update(|v| *v = !*v)
                prop:title=move || i18n::side_status_btn_title(locale.get())
                prop:aria-label=move || i18n::side_status_btn_title(locale.get())
            >
                <svg
                    class="shell-toolbar-icon"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                >
                    <path d="M22 12h-4l-3 9L9 3l-3 9H2" />
                </svg>
            </button>
            <button
                type="button"
                class="btn btn-secondary btn-sm shell-toolbar-icon-btn"
                data-testid="settings-open"
                on:click=move |_| settings_page.set(true)
                prop:title=move || i18n::side_settings_title(locale.get())
                prop:aria-label=move || i18n::side_settings_title(locale.get())
            >
                <svg
                    class="shell-toolbar-icon"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                >
                    <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
                    <circle cx="12" cy="12" r="3" />
                </svg>
            </button>
        </div>
    }
}

#[component]
fn SideColumnShellColumn(
    side_resize_dragging: RwSignal<bool>,
    side_panel_view: RwSignal<SidePanelView>,
    side_width: RwSignal<f64>,
    children: Children,
) -> impl IntoView {
    view! {
        <div
            class:side-column-resizing=move || side_resize_dragging.get()
            class=move || {
                let mut c = String::from("side-column");
                if matches!(side_panel_view.get(), SidePanelView::None) {
                    c.push_str(" side-column-rail-only");
                }
                c
            }
            style:width=move || {
                if matches!(side_panel_view.get(), SidePanelView::None) {
                    "0px".to_string()
                } else {
                    format!("{}px", side_width.get())
                }
            }
        >
            {children()}
        </div>
    }
}

#[component]
pub(super) fn SideColumnResizeAndShellToolbar(
    toolbar: SideColumnResizeToolbarSignals,
    children: Children,
) -> impl IntoView {
    let SideColumnResizeToolbarSignals {
        locale,
        side_resize_dragging,
        side_panel_view,
        side_width,
        side_resize_session,
        side_resize_handles,
        view_menu_open,
        status_bar_visible,
        settings_page,
        status_tasks,
    } = toolbar;
    view! {
        <SideColumnResizeDragHandle
            locale
            side_panel_view
            side_width
            side_resize_dragging
            side_resize_session
            side_resize_handles
        />
        <Show when=move || side_toolbar_rail_float(side_panel_view.get())>
            <SideColumnShellToolbarIcons
                rail_float=true
                locale
                side_panel_view
                view_menu_open
                status_bar_visible
                settings_page
                status_tasks
            />
        </Show>
        <SideColumnShellColumn
            side_resize_dragging
            side_panel_view
            side_width
        >
            <Show when=move || !side_toolbar_rail_float(side_panel_view.get())>
                <SideColumnShellToolbarIcons
                    rail_float=false
                    locale
                    side_panel_view
                    view_menu_open
                    status_bar_visible
                    settings_page
                    status_tasks
                />
            </Show>
            {children()}
        </SideColumnShellColumn>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_prefs::SidePanelView;

    #[test]
    fn side_toolbar_rail_float_only_when_panel_hidden() {
        assert!(side_toolbar_rail_float(SidePanelView::None));
        assert!(!side_toolbar_rail_float(SidePanelView::Workspace));
        assert!(!side_toolbar_rail_float(SidePanelView::Tasks));
        assert!(!side_toolbar_rail_float(SidePanelView::DebugConsole));
    }
}
