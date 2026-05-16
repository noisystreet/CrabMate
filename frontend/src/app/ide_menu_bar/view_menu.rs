//! 「视图」菜单。

use leptos::prelude::*;

use super::menu_id::IdeMenuId;
use super::props::IdeMenuBarSignals;
use crate::i18n;

fn toggle_view_menu(
    open_menu: RwSignal<Option<IdeMenuId>>,
    ide_menubar_dropdown_open: RwSignal<bool>,
) {
    if open_menu.get_untracked() == Some(IdeMenuId::View) {
        open_menu.set(None);
        ide_menubar_dropdown_open.set(false);
    } else {
        open_menu.set(Some(IdeMenuId::View));
        ide_menubar_dropdown_open.set(true);
    }
}

fn close_menus(open_menu: RwSignal<Option<IdeMenuId>>, ide_menubar_dropdown_open: RwSignal<bool>) {
    open_menu.set(None);
    ide_menubar_dropdown_open.set(false);
}

#[component]
pub(super) fn IdeMenuViewSection(
    signals: IdeMenuBarSignals,
    open_menu: RwSignal<Option<IdeMenuId>>,
    ide_menubar_dropdown_open: RwSignal<bool>,
) -> impl IntoView {
    let IdeMenuBarSignals {
        locale,
        editor,
        ide_settings_page,
        ..
    } = signals;

    view! {
        <div class="ide-menu-wrap">
            <button
                type="button"
                class="ide-menu-trigger"
                class:ide-menu-trigger-open=move || open_menu.get() == Some(IdeMenuId::View)
                role="menuitem"
                aria-haspopup="true"
                prop:aria-expanded=move || (open_menu.get() == Some(IdeMenuId::View)).to_string()
                on:click=move |_| toggle_view_menu(open_menu, ide_menubar_dropdown_open)
            >
                {move || i18n::ide_menu_view(locale.get())}
            </button>
            <Show when=move || open_menu.get() == Some(IdeMenuId::View)>
                <div class="ide-menu-dropdown" role="menu">
                    <button
                        type="button"
                        class="ide-menu-item"
                        role="menuitem"
                        on:click=move |_| {
                            ide_settings_page.set(true);
                            close_menus(open_menu, ide_menubar_dropdown_open);
                        }
                    >
                        {move || i18n::ide_menu_editor_settings(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="ide-menu-item ide-menu-item-check"
                        role="menuitemcheckbox"
                        prop:aria-checked=move || editor.line_numbers.get().to_string()
                        on:click=move |_| {
                            editor.line_numbers.update(|v| *v = !*v);
                            close_menus(open_menu, ide_menubar_dropdown_open);
                        }
                    >
                        <span class="ide-menu-check" aria-hidden="true">{move || {
                            if editor.line_numbers.get() { "✓" } else { "" }
                        }}</span>
                        {move || i18n::ide_menu_toggle_line_numbers(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="ide-menu-item ide-menu-item-check"
                        role="menuitemcheckbox"
                        prop:aria-checked=move || editor.word_wrap.get().to_string()
                        on:click=move |_| {
                            editor.word_wrap.update(|v| *v = !*v);
                            close_menus(open_menu, ide_menubar_dropdown_open);
                        }
                    >
                        <span class="ide-menu-check" aria-hidden="true">{move || {
                            if editor.word_wrap.get() { "✓" } else { "" }
                        }}</span>
                        {move || i18n::ide_menu_toggle_word_wrap(locale.get())}
                    </button>
                </div>
            </Show>
        </div>
    }
}
