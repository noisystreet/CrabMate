//! 「编辑」菜单。

use leptos::prelude::*;

use super::menu_id::IdeMenuId;
use super::props::IdeMenuBarSignals;
use crate::i18n;

fn toggle_edit_menu(
    open_menu: RwSignal<Option<IdeMenuId>>,
    ide_menubar_dropdown_open: RwSignal<bool>,
) {
    if open_menu.get_untracked() == Some(IdeMenuId::Edit) {
        open_menu.set(None);
        ide_menubar_dropdown_open.set(false);
    } else {
        open_menu.set(Some(IdeMenuId::Edit));
        ide_menubar_dropdown_open.set(true);
    }
}

fn close_menus(open_menu: RwSignal<Option<IdeMenuId>>, ide_menubar_dropdown_open: RwSignal<bool>) {
    open_menu.set(None);
    ide_menubar_dropdown_open.set(false);
}

#[component]
pub(super) fn IdeMenuEditSection(
    signals: IdeMenuBarSignals,
    open_menu: RwSignal<Option<IdeMenuId>>,
    ide_menubar_dropdown_open: RwSignal<bool>,
) -> impl IntoView {
    let IdeMenuBarSignals {
        locale,
        chrome,
        ide_path,
        ide_load_busy,
        editor_host,
        ..
    } = signals;

    view! {
        <div class="ide-menu-wrap">
            <button
                type="button"
                class="ide-menu-trigger"
                class:ide-menu-trigger-open=move || open_menu.get() == Some(IdeMenuId::Edit)
                role="menuitem"
                aria-haspopup="true"
                prop:aria-expanded=move || (open_menu.get() == Some(IdeMenuId::Edit)).to_string()
                on:click=move |_| toggle_edit_menu(open_menu, ide_menubar_dropdown_open)
            >
                {move || i18n::ide_menu_edit(locale.get())}
            </button>
            <Show when=move || open_menu.get() == Some(IdeMenuId::Edit)>
                <div class="ide-menu-dropdown" role="menu">
                    <button
                        type="button"
                        class="ide-menu-item"
                        role="menuitem"
                        data-testid="ide-menu-find"
                        prop:disabled=move || ide_path.get().is_none() || ide_load_busy.get()
                        on:click=move |_| {
                            chrome.goto_panel_open.set(false);
                            chrome.find_panel_open.set(true);
                            close_menus(open_menu, ide_menubar_dropdown_open);
                        }
                    >
                        {move || i18n::ide_menu_find(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="ide-menu-item"
                        role="menuitem"
                        data-testid="ide-menu-goto-line"
                        prop:disabled=move || ide_path.get().is_none() || ide_load_busy.get()
                        on:click=move |_| {
                            chrome.find_panel_open.set(false);
                            chrome.goto_panel_open.set(true);
                            close_menus(open_menu, ide_menubar_dropdown_open);
                        }
                    >
                        {move || i18n::ide_menu_goto_line(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="ide-menu-item"
                        role="menuitem"
                        prop:disabled=move || ide_path.get().is_none() || ide_load_busy.get()
                        on:click=move |_| {
                            editor_host.select_all();
                            close_menus(open_menu, ide_menubar_dropdown_open);
                        }
                    >
                        {move || i18n::ide_menu_select_all(locale.get())}
                    </button>
                </div>
            </Show>
        </div>
    }
}
