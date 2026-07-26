//! 「文件」菜单（对话顶栏与 IDE 顶栏共用「选择工作区目录」项）。

use leptos::prelude::*;

use super::menu_id::IdeMenuId;
use super::props::IdeMenuBarSignals;
use crate::app::ide_layout_switch::exit_editor_layout;
use crate::app::workspace_root_actions::WorkspaceRootPickHandle;
use crate::i18n::{self, Locale};
use crate::ide_save::{spawn_save_active_tab, spawn_save_all_dirty_tabs};

fn toggle_file_menu(
    open_menu: RwSignal<Option<IdeMenuId>>,
    ide_menubar_dropdown_open: RwSignal<bool>,
) {
    if open_menu.get_untracked() == Some(IdeMenuId::File) {
        open_menu.set(None);
        ide_menubar_dropdown_open.set(false);
    } else {
        open_menu.set(Some(IdeMenuId::File));
        ide_menubar_dropdown_open.set(true);
    }
}

fn close_menus(open_menu: RwSignal<Option<IdeMenuId>>, ide_menubar_dropdown_open: RwSignal<bool>) {
    open_menu.set(None);
    ide_menubar_dropdown_open.set(false);
}

fn on_ide_new_file_click(
    chrome: crate::app::app_signals::IdeChromeSignals,
    open_menu: RwSignal<Option<IdeMenuId>>,
    ide_menubar_dropdown_open: RwSignal<bool>,
) {
    chrome.new_file_path_draft.set(String::new());
    chrome.new_file_modal_open.set(true);
    close_menus(open_menu, ide_menubar_dropdown_open);
}

/// 「选择工作区目录…」菜单项（对话 / 编辑器共用）。
#[component]
pub(crate) fn ShellMenuOpenWorkspaceItem(
    workspace_pick: WorkspaceRootPickHandle,
    open_menu: RwSignal<Option<IdeMenuId>>,
    menubar_dropdown_open: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class="ide-menu-item"
            role="menuitem"
            data-testid="shell-menu-open-workspace"
            prop:disabled=move || workspace_pick.pick_busy_tracked()
            prop:title=move || {
                if workspace_pick.ws.workspace_pick_busy.get() {
                    i18n::ws_browse_busy_title(workspace_pick.locale.get())
                } else {
                    i18n::ws_browse_title(workspace_pick.locale.get())
                }
            }
            on:click=move |_| {
                workspace_pick.spawn_pick_or_reveal();
                close_menus(open_menu, menubar_dropdown_open);
            }
        >
            {move || workspace_pick.menu_label()}
        </button>
    }
}

/// 对话模式顶栏「文件」菜单（仅工作区选择）。
#[component]
pub(crate) fn ChatShellFileMenu(
    locale: RwSignal<Locale>,
    workspace_pick: WorkspaceRootPickHandle,
    open_menu: RwSignal<Option<IdeMenuId>>,
    menubar_dropdown_open: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="shell-topbar-start ide-menu-bar-menus">
            <div class="ide-menu-wrap">
                <button
                    type="button"
                    class="ide-menu-trigger"
                    class:ide-menu-trigger-open=move || open_menu.get() == Some(IdeMenuId::File)
                    role="menuitem"
                    aria-haspopup="true"
                    data-testid="chat-menu-file"
                    prop:aria-expanded=move || (open_menu.get() == Some(IdeMenuId::File)).to_string()
                    on:click=move |_| toggle_file_menu(open_menu, menubar_dropdown_open)
                >
                    {move || i18n::ide_menu_file(locale.get())}
                </button>
                <Show when=move || open_menu.get() == Some(IdeMenuId::File)>
                    <div class="ide-menu-dropdown" role="menu">
                        <ShellMenuOpenWorkspaceItem
                            workspace_pick=workspace_pick
                            open_menu=open_menu
                            menubar_dropdown_open=menubar_dropdown_open
                        />
                    </div>
                </Show>
            </div>
        </div>
        <Show when=move || open_menu.get().is_some()>
            <button
                type="button"
                class="ide-menu-backdrop"
                tabindex="-1"
                aria-hidden="true"
                on:click=move |_| {
                    open_menu.set(None);
                    menubar_dropdown_open.set(false);
                }
            />
        </Show>
    }
}

#[component]
pub(super) fn IdeMenuFileSection(
    signals: IdeMenuBarSignals,
    open_menu: RwSignal<Option<IdeMenuId>>,
    ide_menubar_dropdown_open: RwSignal<bool>,
    save_enabled: Memo<bool>,
    save_all_enabled: Memo<bool>,
) -> impl IntoView {
    let IdeMenuBarSignals {
        locale,
        chrome,
        layout_toggle,
        ide_load_busy,
        ide_save_busy,
        save_ctx,
        workspace_pick,
        ..
    } = signals;

    view! {
        <div class="ide-menu-wrap">
            <button
                type="button"
                class="ide-menu-trigger"
                class:ide-menu-trigger-open=move || open_menu.get() == Some(IdeMenuId::File)
                role="menuitem"
                aria-haspopup="true"
                prop:aria-expanded=move || (open_menu.get() == Some(IdeMenuId::File)).to_string()
                on:click=move |_| toggle_file_menu(open_menu, ide_menubar_dropdown_open)
            >
                {move || i18n::ide_menu_file(locale.get())}
            </button>
            <Show when=move || open_menu.get() == Some(IdeMenuId::File)>
                <div class="ide-menu-dropdown" role="menu">
                    <ShellMenuOpenWorkspaceItem
                        workspace_pick=workspace_pick
                        open_menu=open_menu
                        menubar_dropdown_open=ide_menubar_dropdown_open
                    />
                    <button
                        type="button"
                        class="ide-menu-item"
                        role="menuitem"
                        data-testid="ide-menu-new-file"
                        prop:disabled=move || ide_load_busy.get() || ide_save_busy.get()
                        on:click=move |_| {
                            on_ide_new_file_click(chrome, open_menu, ide_menubar_dropdown_open);
                        }
                    >
                        {move || i18n::ide_menu_new_file(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="ide-menu-item"
                        role="menuitem"
                        data-testid="ide-menu-save"
                        prop:disabled=move || !save_enabled.get()
                        on:click=move |_| {
                            spawn_save_active_tab(save_ctx, locale);
                            close_menus(open_menu, ide_menubar_dropdown_open);
                        }
                    >
                        {move || {
                            if ide_save_busy.get() {
                                i18n::ide_saving(locale.get())
                            } else {
                                i18n::ide_menu_save(locale.get())
                            }
                        }}
                    </button>
                    <button
                        type="button"
                        class="ide-menu-item"
                        role="menuitem"
                        prop:disabled=move || !save_all_enabled.get()
                        on:click=move |_| {
                            spawn_save_all_dirty_tabs(save_ctx, locale);
                            close_menus(open_menu, ide_menubar_dropdown_open);
                        }
                    >
                        {move || i18n::ide_menu_save_all(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="ide-menu-item"
                        role="menuitem"
                        data-testid="ide-menu-back-to-chat"
                        on:click=move |_| {
                            exit_editor_layout(layout_toggle);
                            close_menus(open_menu, ide_menubar_dropdown_open);
                        }
                    >
                        {move || i18n::ide_menu_back_to_chat(locale.get())}
                    </button>
                </div>
            </Show>
        </div>
    }
}
