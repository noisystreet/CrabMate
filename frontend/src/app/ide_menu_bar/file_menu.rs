//! 「文件」菜单。

use leptos::prelude::*;
use leptos::task::spawn_local;

use super::menu_id::IdeMenuId;
use super::props::IdeMenuBarSignals;
use crate::api::post_workspace_file_write;
use crate::i18n;

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

#[component]
pub(super) fn IdeMenuFileSection(
    signals: IdeMenuBarSignals,
    open_menu: RwSignal<Option<IdeMenuId>>,
    ide_menubar_dropdown_open: RwSignal<bool>,
    save_enabled: Memo<bool>,
) -> impl IntoView {
    let IdeMenuBarSignals {
        locale,
        editor_layout_mode,
        ide_path,
        ide_text,
        ide_baseline,
        ide_load_busy,
        ide_save_busy,
        ide_err,
        tabs,
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
                        prop:disabled=move || !save_enabled.get()
                        on:click=move |_| {
                            let Some(p) = ide_path.get_untracked() else {
                                return;
                            };
                            if ide_load_busy.get_untracked() || ide_save_busy.get_untracked() {
                                return;
                            }
                            ide_save_busy.set(true);
                            ide_err.set(None);
                            let body = ide_text.get_untracked();
                            let loc = locale.get_untracked();
                            spawn_local(async move {
                                match post_workspace_file_write(p, body, loc).await {
                                    Ok(()) => {
                                        let snap = ide_text.get_untracked();
                                        ide_baseline.set(snap.clone());
                                        if let Some(i) = tabs.active.get_untracked() {
                                            tabs.tabs.update(|list| {
                                                if let Some(tab) = list.get_mut(i) {
                                                    tab.text = snap.clone();
                                                    tab.baseline = snap;
                                                }
                                            });
                                        }
                                    }
                                    Err(e) => ide_err.set(Some(e)),
                                }
                                ide_save_busy.set(false);
                            });
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
