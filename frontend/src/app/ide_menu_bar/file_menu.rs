//! 「文件」菜单。

use leptos::prelude::*;

use super::menu_id::IdeMenuId;
use super::props::IdeMenuBarSignals;
use crate::i18n::{self};
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
        editor_layout_mode,
        ide_load_busy,
        ide_save_busy,
        save_ctx,
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
                            editor_layout_mode.set(false);
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
