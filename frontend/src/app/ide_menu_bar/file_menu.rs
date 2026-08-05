//! 「文件」菜单（对话顶栏与 IDE 顶栏共用：打开工作区、最近；IDE 另附保存/新建/回会话）。

use leptos::prelude::*;

use super::menu_id::IdeMenuId;
use super::props::IdeMenuBarSignals;
use crate::app::app_signals::IdeChromeSignals;
use crate::app::ide_layout_switch::{IdeLayoutToggleSignals, exit_editor_layout};
use crate::app::workspace_root_actions::WorkspaceRootPickHandle;
use crate::i18n::{self, Locale};
use crate::ide_save::{IdeSaveContext, spawn_save_active_tab, spawn_save_all_dirty_tabs};
use crate::user_data_bootstrap::workspace_recent_menu_label;

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
    chrome: IdeChromeSignals,
    open_menu: RwSignal<Option<IdeMenuId>>,
    ide_menubar_dropdown_open: RwSignal<bool>,
) {
    chrome.new_file_path_draft.set(String::new());
    chrome.new_file_modal_open.set(true);
    close_menus(open_menu, ide_menubar_dropdown_open);
}

/// IDE「文件」菜单额外项所需信号（会话模式不传）。
#[derive(Clone, Copy)]
pub(crate) struct ShellTopbarFileMenuIde {
    pub chrome: IdeChromeSignals,
    pub layout_toggle: IdeLayoutToggleSignals,
    pub ide_load_busy: RwSignal<bool>,
    pub ide_save_busy: RwSignal<bool>,
    pub save_ctx: IdeSaveContext,
    pub save_enabled: Memo<bool>,
    pub save_all_enabled: Memo<bool>,
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

/// 「最近的工作区」级联子菜单（右侧飞出列表）。
#[component]
pub(crate) fn ShellMenuRecentWorkspaces(
    workspace_pick: WorkspaceRootPickHandle,
    open_menu: RwSignal<Option<IdeMenuId>>,
    menubar_dropdown_open: RwSignal<bool>,
    /// IDE 菜单在「最近」与「新建文件」之间再画一条分隔线。
    #[prop(optional)]
    trailing_separator: bool,
) -> impl IntoView {
    let recent = workspace_pick.ws.recent_workspace_roots;
    let locale = workspace_pick.locale;
    let submenu_open = RwSignal::new(false);

    Effect::new(move |_| {
        if open_menu.get() != Some(IdeMenuId::File) {
            submenu_open.set(false);
        }
    });

    view! {
        <Show when=move || !recent.get().is_empty()>
            <div
                class="ide-menu-submenu"
                class:ide-menu-submenu-open=move || submenu_open.get()
                data-testid="shell-menu-recent-workspaces"
                on:mouseenter=move |_| submenu_open.set(true)
                on:mouseleave=move |_| submenu_open.set(false)
            >
                <button
                    type="button"
                    class="ide-menu-item ide-menu-submenu-trigger"
                    role="menuitem"
                    aria-haspopup="true"
                    prop:aria-expanded=move || submenu_open.get().to_string()
                    prop:disabled=move || workspace_pick.pick_busy_tracked()
                    on:click=move |ev| {
                        ev.stop_propagation();
                        submenu_open.update(|o| *o = !*o);
                    }
                >
                    <span class="ide-menu-submenu-label">
                        {move || i18n::ide_menu_recent_workspaces(locale.get())}
                    </span>
                    <span class="ide-menu-submenu-chevron" aria-hidden="true">"›"</span>
                </button>
                <Show when=move || submenu_open.get()>
                    <div class="ide-menu-submenu-flyout" role="menu">
                        <For
                            each=move || recent.get()
                            key=|p| p.clone()
                            let:path
                        >
                            {
                                let path_for_click = path.clone();
                                let path_for_title = path.clone();
                                let path_for_label = path.clone();
                                let path_for_test = path.clone();
                                view! {
                                    <button
                                        type="button"
                                        class="ide-menu-item ide-menu-item-recent"
                                        role="menuitem"
                                        data-testid="shell-menu-recent-workspace"
                                        prop:data-path=path_for_test
                                        prop:disabled=move || workspace_pick.pick_busy_tracked()
                                        prop:title=path_for_title
                                        on:click=move |_| {
                                            workspace_pick.spawn_open_recent(path_for_click.clone());
                                            submenu_open.set(false);
                                            close_menus(open_menu, menubar_dropdown_open);
                                        }
                                    >
                                        {workspace_recent_menu_label(&path_for_label)}
                                    </button>
                                }
                            }
                        </For>
                    </div>
                </Show>
            </div>
            <Show when=move || trailing_separator>
                <div class="ide-menu-separator" role="separator" />
            </Show>
        </Show>
    }
}

#[component]
fn ShellTopbarFileMenuIdeItems(
    locale: RwSignal<Locale>,
    open_menu: RwSignal<Option<IdeMenuId>>,
    menubar_dropdown_open: RwSignal<bool>,
    ide: ShellTopbarFileMenuIde,
) -> impl IntoView {
    let ShellTopbarFileMenuIde {
        chrome,
        layout_toggle,
        ide_load_busy,
        ide_save_busy,
        save_ctx,
        save_enabled,
        save_all_enabled,
    } = ide;

    view! {
        <button
            type="button"
            class="ide-menu-item"
            role="menuitem"
            data-testid="ide-menu-new-file"
            prop:disabled=move || ide_load_busy.get() || ide_save_busy.get()
            on:click=move |_| {
                on_ide_new_file_click(chrome, open_menu, menubar_dropdown_open);
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
                close_menus(open_menu, menubar_dropdown_open);
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
                close_menus(open_menu, menubar_dropdown_open);
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
                close_menus(open_menu, menubar_dropdown_open);
            }
        >
            {move || i18n::ide_menu_back_to_chat(locale.get())}
        </button>
    }
}

/// 会话 / IDE 共用的「文件」下拉（工作区项 + 可选 IDE 动作）。
#[component]
pub(crate) fn ShellTopbarFileMenu(
    locale: RwSignal<Locale>,
    workspace_pick: WorkspaceRootPickHandle,
    open_menu: RwSignal<Option<IdeMenuId>>,
    menubar_dropdown_open: RwSignal<bool>,
    #[prop(optional)] ide: Option<ShellTopbarFileMenuIde>,
) -> impl IntoView {
    let has_ide = ide.is_some();
    let trigger_testid = if has_ide {
        "ide-menu-file"
    } else {
        "chat-menu-file"
    };

    view! {
        <div class="ide-menu-wrap">
            <button
                type="button"
                class="ide-menu-trigger"
                class:ide-menu-trigger-open=move || open_menu.get() == Some(IdeMenuId::File)
                role="menuitem"
                aria-haspopup="true"
                data-testid=trigger_testid
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
                    <ShellMenuRecentWorkspaces
                        workspace_pick=workspace_pick
                        open_menu=open_menu
                        menubar_dropdown_open=menubar_dropdown_open
                        trailing_separator=has_ide
                    />
                    {ide.map(|ide| {
                        view! {
                            <ShellTopbarFileMenuIdeItems
                                locale=locale
                                open_menu=open_menu
                                menubar_dropdown_open=menubar_dropdown_open
                                ide=ide
                            />
                        }
                    })}
                </div>
            </Show>
        </div>
    }
}

/// 菜单下拉打开时的全屏透明遮罩（会话 / IDE 共用）。
#[component]
pub(crate) fn ShellTopbarMenuBackdrop(
    open_menu: RwSignal<Option<IdeMenuId>>,
    menubar_dropdown_open: RwSignal<bool>,
) -> impl IntoView {
    view! {
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
        <ShellTopbarFileMenu
            locale=locale
            workspace_pick=workspace_pick
            open_menu=open_menu
            menubar_dropdown_open=ide_menubar_dropdown_open
            ide=ShellTopbarFileMenuIde {
                chrome,
                layout_toggle,
                ide_load_busy,
                ide_save_busy,
                save_ctx,
                save_enabled,
                save_all_enabled,
            }
        />
    }
}
