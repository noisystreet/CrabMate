//! 「文件」菜单。

use leptos::prelude::*;

use super::menu_id::IdeMenuId;
use super::props::IdeMenuBarSignals;
use crate::i18n;
use crate::ide_save::{
    IdeSaveContext, prompt_new_workspace_file_path, spawn_create_and_open_file,
    spawn_save_active_tab, spawn_save_all_dirty_tabs,
};

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

fn save_ctx(signals: IdeMenuBarSignals) -> IdeSaveContext {
    let IdeMenuBarSignals {
        tabs,
        ide_path,
        ide_text,
        ide_baseline,
        ide_err,
        ..
    } = signals;
    IdeSaveContext {
        tabs,
        ide_path,
        ide_text,
        ide_baseline,
        ide_err,
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
        editor_layout_mode,
        ide_path: _,
        ide_text: _,
        ide_baseline: _,
        ide_load_busy,
        ide_save_busy,
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
                        prop:disabled=move || ide_load_busy.get() || ide_save_busy.get()
                        on:click=move |_| {
                            let loc = locale.get_untracked();
                            if let Some(rel) = prompt_new_workspace_file_path(loc) {
                                spawn_create_and_open_file(
                                    save_ctx(signals),
                                    locale,
                                    rel,
                                );
                            }
                            close_menus(open_menu, ide_menubar_dropdown_open);
                        }
                    >
                        {move || i18n::ide_menu_new_file(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="ide-menu-item"
                        role="menuitem"
                        prop:disabled=move || !save_enabled.get()
                        on:click=move |_| {
                            spawn_save_active_tab(save_ctx(signals), locale);
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
                            spawn_save_all_dirty_tabs(save_ctx(signals), locale);
                            close_menus(open_menu, ide_menubar_dropdown_open);
                        }
                    >
                        {move || i18n::ide_menu_save_all(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="ide-menu-item"
                        role="menuitem"
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
