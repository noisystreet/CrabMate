//! 对话 / 编辑器布局切换：侧栏状态恢复与退出 IDE 时的宽度过渡抑制。

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

/// 布局切换相关信号（进入 IDE 时记住侧栏展开/收起，退出时恢复且避免宽度过渡闪动）。
#[derive(Clone, Copy)]
pub struct IdeLayoutToggleSignals {
    pub editor_layout_mode: RwSignal<bool>,
    pub sidebar_rail_collapsed: RwSignal<bool>,
    pub sidebar_rail_snap: RwSignal<bool>,
    pub sidebar_collapsed_before_ide: RwSignal<Option<bool>>,
}

impl IdeLayoutToggleSignals {
    pub fn from_app_signals(app: &crate::app::app_signals::AppSignals) -> Self {
        Self {
            editor_layout_mode: app.shell_ui.editor_layout_mode,
            sidebar_rail_collapsed: app.sidebar.sidebar_rail_collapsed,
            sidebar_rail_snap: app.sidebar.sidebar_rail_snap,
            sidebar_collapsed_before_ide: app.sidebar.sidebar_collapsed_before_ide,
        }
    }
}

fn clear_sidebar_rail_snap_after_paint(snap: RwSignal<bool>) {
    let Some(win) = web_sys::window() else {
        snap.set(false);
        return;
    };
    let win2 = win.clone();
    let cb = Closure::once(Box::new(move || {
        let cb2 = Closure::once(Box::new(move || snap.set(false)));
        let _ = win2.request_animation_frame(cb2.as_ref().unchecked_ref());
        cb2.forget();
    }));
    let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
    cb.forget();
}

/// 进入 IDE 时记住当前侧栏收起状态（由 Effect 调用）。
pub fn remember_sidebar_before_ide(s: IdeLayoutToggleSignals) {
    if s.sidebar_collapsed_before_ide.get_untracked().is_none() {
        s.sidebar_collapsed_before_ide
            .set(Some(s.sidebar_rail_collapsed.get_untracked()));
    }
}

/// 退出编辑器布局：先恢复侧栏并禁用宽度过渡，再关闭 IDE 模式。
pub fn exit_editor_layout(s: IdeLayoutToggleSignals) {
    s.sidebar_rail_snap.set(true);
    if let Some(was) = s.sidebar_collapsed_before_ide.get_untracked() {
        s.sidebar_rail_collapsed.set(was);
        s.sidebar_collapsed_before_ide.set(None);
    }
    s.editor_layout_mode.set(false);
    clear_sidebar_rail_snap_after_paint(s.sidebar_rail_snap);
}

pub fn enter_editor_layout(s: IdeLayoutToggleSignals) {
    s.editor_layout_mode.set(true);
}

pub fn toggle_editor_layout(s: IdeLayoutToggleSignals) {
    if s.editor_layout_mode.get_untracked() {
        exit_editor_layout(s);
    } else {
        enter_editor_layout(s);
    }
}
