//! 窄屏视口检测：维护 `is_narrow_viewport`、DOM `data-narrow-viewport`，进入窄屏时暂存并收起右侧面板。

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use crate::app::ide_layout_switch::{IdeLayoutToggleSignals, exit_editor_layout};
use crate::app::shell_prefs_storage;
use crate::app_prefs::{SidePanelView, mobile_layout_media_query};
use crate::mobile_remote::mobile_remote_client;

pub struct WireNarrowViewportSignals {
    pub is_narrow_viewport: RwSignal<bool>,
    pub side_panel_view: RwSignal<SidePanelView>,
    pub layout_toggle: IdeLayoutToggleSignals,
}

fn force_session_layout_if_mobile(layout_toggle: IdeLayoutToggleSignals, narrow: bool) {
    if !layout_toggle.editor_layout_mode.get_untracked() {
        return;
    }
    if narrow || mobile_remote_client() {
        exit_editor_layout(layout_toggle);
    }
}

fn media_query_matches(query: &str) -> Option<bool> {
    let window = web_sys::window()?;
    let f = js_sys::Reflect::get(
        window.as_ref(),
        &wasm_bindgen::JsValue::from_str("matchMedia"),
    )
    .ok()?;
    let f = f.dyn_into::<js_sys::Function>().ok()?;
    let mql_v = f
        .call1(window.as_ref(), &wasm_bindgen::JsValue::from_str(query))
        .ok()?;
    if mql_v.is_null() || mql_v.is_undefined() {
        return None;
    }
    let mql = mql_v.dyn_into::<js_sys::Object>().ok()?;
    js_sys::Reflect::get(&mql, &wasm_bindgen::JsValue::from_str("matches"))
        .ok()
        .and_then(|v| v.as_bool())
}

fn on_viewport_narrow_change(
    matches: bool,
    is_narrow_viewport: RwSignal<bool>,
    side_panel_view: RwSignal<SidePanelView>,
    stashed_panel: StoredValue<Option<SidePanelView>>,
    layout_toggle: IdeLayoutToggleSignals,
) {
    let was_narrow = is_narrow_viewport.get_untracked();
    if matches == was_narrow {
        return;
    }
    is_narrow_viewport.set(matches);
    shell_prefs_storage::apply_narrow_viewport_dom_flag(matches);
    if matches || mobile_remote_client() {
        force_session_layout_if_mobile(layout_toggle, true);
        let current = side_panel_view.get_untracked();
        if !matches!(current, SidePanelView::None) {
            if !mobile_remote_client() {
                stashed_panel.set_value(Some(current));
            }
            side_panel_view.set(SidePanelView::None);
        }
    } else if let Some(prev) = stashed_panel.get_value() {
        side_panel_view.set(prev);
        stashed_panel.set_value(None);
    }
    // 无暂存时保留当前 prefs（勿强制 Workspace，以免覆盖用户隐藏）。
}

pub fn wire_narrow_viewport_layout(sig: WireNarrowViewportSignals) {
    let is_narrow_viewport = sig.is_narrow_viewport;
    let side_panel_view = sig.side_panel_view;
    let layout_toggle = sig.layout_toggle;
    let stashed_panel = StoredValue::new(None::<SidePanelView>);

    // 窄屏 / Android 壳：锁定会话布局（偏好水合后若仍为 IDE 也会被拉回）
    Effect::new(move |_| {
        let narrow = is_narrow_viewport.get();
        let _in_ide = layout_toggle.editor_layout_mode.get();
        force_session_layout_if_mobile(layout_toggle, narrow);
    });

    Effect::new(move |_| {
        let Some(window) = web_sys::window() else {
            return;
        };
        let query = mobile_layout_media_query();
        if let Some(initial) = media_query_matches(query.as_str()) {
            let collapse = initial || mobile_remote_client();
            if collapse {
                force_session_layout_if_mobile(layout_toggle, true);
                let current = side_panel_view.get_untracked();
                if !matches!(current, SidePanelView::None) {
                    if !mobile_remote_client() {
                        stashed_panel.set_value(Some(current));
                    }
                    side_panel_view.set(SidePanelView::None);
                }
            } else {
                // 桌面宽屏首屏：工作区展开（prefs 水合后还会再按端覆盖一次）
                if matches!(side_panel_view.get_untracked(), SidePanelView::None) {
                    side_panel_view.set(SidePanelView::Workspace);
                }
            }
            is_narrow_viewport.set(initial);
            shell_prefs_storage::apply_narrow_viewport_dom_flag(initial);
        } else if mobile_remote_client() {
            side_panel_view.set(SidePanelView::None);
        }

        let Ok(f) = js_sys::Reflect::get(
            window.as_ref(),
            &wasm_bindgen::JsValue::from_str("matchMedia"),
        ) else {
            return;
        };
        let Ok(f) = f.dyn_into::<js_sys::Function>() else {
            return;
        };
        let Ok(mql_v) = f.call1(
            window.as_ref(),
            &wasm_bindgen::JsValue::from_str(query.as_str()),
        ) else {
            return;
        };
        if mql_v.is_null() || mql_v.is_undefined() {
            return;
        }
        let Ok(mql) = mql_v.dyn_into::<web_sys::EventTarget>() else {
            return;
        };

        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            if let Some(matches) = media_query_matches(query.as_str()) {
                on_viewport_narrow_change(
                    matches,
                    is_narrow_viewport,
                    side_panel_view,
                    stashed_panel,
                    layout_toggle,
                );
            }
        });
        let _ = mql.add_event_listener_with_callback("change", cb.as_ref().unchecked_ref());
        cb.forget();
    });
}
