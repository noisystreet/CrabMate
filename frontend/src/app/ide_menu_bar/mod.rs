//! IDE 顶栏菜单（文件 / 编辑 / 视图），替代原编辑器工具条。

mod edit_menu;
mod file_menu;
mod menu_id;
mod props;
mod view_menu;

pub use props::IdeMenuBarSignals;

use edit_menu::IdeMenuEditSection;
use file_menu::IdeMenuFileSection;
use leptos::prelude::*;
use menu_id::IdeMenuId;
use view_menu::IdeMenuViewSection;

use crate::app::tauri_window_controls::TauriWindowControls;
use crate::i18n;

#[component]
pub fn IdeMenuBar(signals: IdeMenuBarSignals) -> impl IntoView {
    let IdeMenuBarSignals {
        locale,
        ide_menubar_dropdown_open,
        ide_path,
        ide_text,
        ide_baseline,
        ide_load_busy,
        ide_save_busy,
        tabs,
        ..
    } = signals;

    let open_menu = RwSignal::new(None::<IdeMenuId>);

    let close_menu = move || {
        open_menu.set(None);
        ide_menubar_dropdown_open.set(false);
    };

    Effect::new(move |_| {
        if !ide_menubar_dropdown_open.get() {
            open_menu.set(None);
        }
    });

    let save_enabled = Memo::new(move |_| {
        ide_path.get().is_some()
            && !ide_load_busy.get()
            && !ide_save_busy.get()
            && ide_text.get() != ide_baseline.get()
    });

    let save_all_enabled = Memo::new(move |_| {
        if ide_load_busy.get() || ide_save_busy.get() {
            return false;
        }
        let active = tabs.active.get();
        let dirty_inactive = tabs.tabs.get().iter().enumerate().any(|(i, tab)| {
            if active == Some(i) {
                return false;
            }
            tab.text != tab.baseline
        });
        dirty_inactive || save_enabled.get()
    });

    view! {
        <header class="ide-menu-bar" role="menubar" prop:aria-label=move || i18n::ide_menu_bar_aria(locale.get())>
            <div class="ide-menu-bar-menus">
                <IdeMenuFileSection
                    signals=signals
                    open_menu=open_menu
                    ide_menubar_dropdown_open=ide_menubar_dropdown_open
                    save_enabled=save_enabled
                    save_all_enabled=save_all_enabled
                />
                <IdeMenuEditSection
                    signals=signals
                    open_menu=open_menu
                    ide_menubar_dropdown_open=ide_menubar_dropdown_open
                />
                <IdeMenuViewSection
                    signals=signals
                    open_menu=open_menu
                    ide_menubar_dropdown_open=ide_menubar_dropdown_open
                />
            </div>
            <div class="ide-menu-bar-status">
                <Show when=move || ide_text.get() != ide_baseline.get()>
                    <span class="ide-dirty-dot" aria-hidden="true">"●"</span>
                </Show>
                <span class="ide-menu-bar-path">{move || ide_path.get().unwrap_or_default()}</span>
            </div>
            <TauriWindowControls locale=locale />
            <Show when=move || open_menu.get().is_some()>
                <button
                    type="button"
                    class="ide-menu-backdrop"
                    tabindex="-1"
                    aria-hidden="true"
                    on:click=move |_| close_menu()
                />
            </Show>
        </header>
    }
}
