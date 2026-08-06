//! IDE 顶栏菜单（项目 / 编辑 / 视图），渲染于统一壳顶栏。

mod edit_menu;
mod file_menu;
mod menu_id;
mod props;
mod view_menu;

pub use props::{IdeMenuBarBridge, IdeMenuBarSignals};

pub(crate) use file_menu::{ShellTopbarFileMenu, ShellTopbarMenuBackdrop};
pub(crate) use menu_id::IdeMenuId;

use edit_menu::IdeMenuEditSection;
use file_menu::IdeMenuFileSection;
use leptos::prelude::*;
use menu_id::IdeMenuId as MenuId;
use view_menu::IdeMenuViewSection;

/// IDE 模式顶栏菜单：项目 / 编辑 / 视图（工作区根与当前文件路径由统一壳顶栏渲染）。
#[component]
pub fn IdeMenuBarTopbarContent(bridge: IdeMenuBarBridge) -> impl IntoView {
    let IdeMenuBarBridge {
        signals,
        open_menu,
        save_enabled,
        save_all_enabled,
    } = bridge;

    let IdeMenuBarSignals {
        ide_menubar_dropdown_open,
        ..
    } = signals;

    view! {
        <div class="shell-topbar-start shell-topbar-menus">
            <IdeMenuFileSection
                signals=signals
                open_menu=open_menu
                ide_menubar_dropdown_open=signals.ide_menubar_dropdown_open
                save_enabled=save_enabled
                save_all_enabled=save_all_enabled
            />
            <IdeMenuEditSection
                signals=signals
                open_menu=open_menu
                ide_menubar_dropdown_open=signals.ide_menubar_dropdown_open
            />
            <IdeMenuViewSection
                signals=signals
                open_menu=open_menu
                ide_menubar_dropdown_open=signals.ide_menubar_dropdown_open
            />
        </div>
        <ShellTopbarMenuBackdrop
            open_menu=open_menu
            menubar_dropdown_open=ide_menubar_dropdown_open
        />
    }
}

/// 注册 IDE 顶栏桥接状态，并在布局卸载时清除。
pub fn wire_ide_menu_bar_bridge(
    bridge_slot: RwSignal<Option<IdeMenuBarBridge>>,
    editor_visible: RwSignal<bool>,
    signals: IdeMenuBarSignals,
) {
    let open_menu = RwSignal::new(None::<MenuId>);

    Effect::new(move |_| {
        if !signals.ide_menubar_dropdown_open.get() {
            open_menu.set(None);
        }
    });

    let save_enabled = Memo::new(move |_| {
        signals.ide_path.get().is_some()
            && !signals.ide_load_busy.get()
            && !signals.ide_save_busy.get()
            && signals.ide_text.get() != signals.ide_baseline.get()
    });

    let save_all_enabled = Memo::new(move |_| {
        if signals.ide_load_busy.get() || signals.ide_save_busy.get() {
            return false;
        }
        let active = signals.tabs.active.get();
        let dirty_inactive = signals.tabs.tabs.get().iter().enumerate().any(|(i, tab)| {
            if active == Some(i) {
                return false;
            }
            tab.text != tab.baseline
        });
        dirty_inactive || save_enabled.get()
    });

    let sync_bridge = move || {
        if editor_visible.get_untracked() {
            bridge_slot.set(Some(IdeMenuBarBridge {
                signals,
                open_menu,
                save_enabled,
                save_all_enabled,
            }));
        } else {
            bridge_slot.set(None);
        }
    };

    sync_bridge();

    Effect::new(move |_| {
        let _ = editor_visible.get();
        sync_bridge();
    });
}
