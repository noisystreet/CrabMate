//! 窄屏：自屏幕左缘右划打开会话列表（`nav-rail`），打开后再左划关闭。

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use crate::app_prefs::SidePanelView;

/// 左缘感应宽度（CSS px）。
const EDGE_ZONE_PX: f64 = 28.0;
/// 水平位移超过此值才判定为开/关手势。
const SWIPE_THRESHOLD_PX: f64 = 56.0;
/// |dx| 须明显大于 |dy|，避免与纵向滚动冲突。
const HORIZ_DOMINANCE: f64 = 1.15;

#[derive(Clone, Copy)]
pub struct WireMobileNavEdgeSwipeSignals {
    pub is_narrow_viewport: RwSignal<bool>,
    pub mobile_nav_open: RwSignal<bool>,
    /// 打开会话栏时关闭右侧抽屉（互斥）。
    pub side_panel_view: RwSignal<SidePanelView>,
}

#[derive(Clone, Copy)]
struct TouchOrigin {
    x: f64,
    y: f64,
    /// `true`：侧栏已开，手势用于关闭；`false`：自左缘开启。
    closing: bool,
}

fn overlay_blocks_swipe(doc: &web_sys::Document) -> bool {
    doc.query_selector(
        ".modal-backdrop, .settings-page-visible, .changelist-modal-layer, .session-ctx-menu",
    )
    .ok()
    .flatten()
    .is_some()
}

fn target_is_text_field(ev: &web_sys::PointerEvent) -> bool {
    let Some(t) = ev.target() else {
        return false;
    };
    let Ok(el) = t.dyn_into::<web_sys::Element>() else {
        return false;
    };
    let tag = el.tag_name().to_ascii_lowercase();
    if tag == "input" || tag == "textarea" || tag == "select" {
        return true;
    }
    el.closest("[contenteditable='true']")
        .ok()
        .flatten()
        .is_some()
}

fn side_drawer_open(view: SidePanelView) -> bool {
    !matches!(view, SidePanelView::None)
}

fn maybe_complete_swipe(
    origin: &Cell<Option<TouchOrigin>>,
    narrow: RwSignal<bool>,
    nav_open: RwSignal<bool>,
    side_panel_view: RwSignal<SidePanelView>,
    ev: &web_sys::PointerEvent,
) {
    let Some(start) = origin.take() else {
        return;
    };
    if !narrow.get_untracked() {
        return;
    }
    let dx = f64::from(ev.client_x()) - start.x;
    let dy = f64::from(ev.client_y()) - start.y;
    if dx.abs() < SWIPE_THRESHOLD_PX {
        return;
    }
    if dx.abs() < dy.abs() * HORIZ_DOMINANCE {
        return;
    }
    if start.closing {
        if dx <= -SWIPE_THRESHOLD_PX {
            nav_open.set(false);
        }
    } else if dx >= SWIPE_THRESHOLD_PX {
        side_panel_view.set(SidePanelView::None);
        nav_open.set(true);
    }
}

/// 窄屏下挂接 pointer 边缘滑动：右划开会话侧栏，左划关。
pub fn wire_mobile_nav_edge_swipe(sig: WireMobileNavEdgeSwipeSignals) {
    Effect::new(move |_| {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Some(doc) = window.document() else {
            return;
        };

        let origin: Rc<Cell<Option<TouchOrigin>>> = Rc::new(Cell::new(None));
        let narrow = sig.is_narrow_viewport;
        let nav_open = sig.mobile_nav_open;
        let side_panel_view = sig.side_panel_view;

        let origin_down = Rc::clone(&origin);
        let on_down = Closure::<dyn FnMut(web_sys::PointerEvent)>::new({
            let doc = doc.clone();
            move |ev: web_sys::PointerEvent| {
                if !narrow.get_untracked() {
                    origin_down.set(None);
                    return;
                }
                if ev.pointer_type() == "mouse" && ev.button() != 0 {
                    return;
                }
                if overlay_blocks_swipe(&doc) || target_is_text_field(&ev) {
                    origin_down.set(None);
                    return;
                }
                // 右侧抽屉打开时不抢左缘开启手势（仍允许在已开会话栏上左划关闭）
                let open = nav_open.get_untracked();
                if !open && side_drawer_open(side_panel_view.get_untracked()) {
                    origin_down.set(None);
                    return;
                }
                let x = f64::from(ev.client_x());
                let y = f64::from(ev.client_y());
                if open {
                    origin_down.set(Some(TouchOrigin {
                        x,
                        y,
                        closing: true,
                    }));
                } else if x <= EDGE_ZONE_PX {
                    origin_down.set(Some(TouchOrigin {
                        x,
                        y,
                        closing: false,
                    }));
                } else {
                    origin_down.set(None);
                }
            }
        });

        let origin_up = Rc::clone(&origin);
        let on_up =
            Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |ev: web_sys::PointerEvent| {
                maybe_complete_swipe(&origin_up, narrow, nav_open, side_panel_view, &ev);
            });

        let origin_cancel = Rc::clone(&origin);
        let on_cancel =
            Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |ev: web_sys::PointerEvent| {
                maybe_complete_swipe(&origin_cancel, narrow, nav_open, side_panel_view, &ev);
            });

        let _ =
            doc.add_event_listener_with_callback("pointerdown", on_down.as_ref().unchecked_ref());
        let _ = doc.add_event_listener_with_callback("pointerup", on_up.as_ref().unchecked_ref());
        let _ = doc
            .add_event_listener_with_callback("pointercancel", on_cancel.as_ref().unchecked_ref());

        on_down.forget();
        on_up.forget();
        on_cancel.forget();
    });
}
