//! 窄屏：自屏幕右缘左划打开右侧工作区抽屉（`side-column`），打开后再右划关闭。
//! 与左缘会话栏互斥；默认恢复上次非 `None` 的面板（否则工作区）。

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use crate::app_prefs::SidePanelView;

/// 右缘感应宽度（CSS px）。
const EDGE_ZONE_PX: f64 = 28.0;
/// 水平位移超过此值才判定为开/关手势。
const SWIPE_THRESHOLD_PX: f64 = 56.0;
/// |dx| 须明显大于 |dy|，避免与纵向滚动冲突。
const HORIZ_DOMINANCE: f64 = 1.15;

#[derive(Clone, Copy)]
pub struct WireMobileSideEdgeSwipeSignals {
    pub is_narrow_viewport: RwSignal<bool>,
    pub side_panel_view: RwSignal<SidePanelView>,
    pub mobile_nav_open: RwSignal<bool>,
}

#[derive(Clone, Copy)]
struct TouchOrigin {
    x: f64,
    y: f64,
    /// `true`：右栏已开，手势用于关闭；`false`：自右缘开启。
    closing: bool,
}

fn overlay_blocks_swipe(doc: &web_sys::Document) -> bool {
    doc.query_selector(
        ".modal-backdrop, .settings-page-visible, .changelist-modal-layer, .session-ctx-menu, .nav-rail-mobile-open",
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

fn side_panel_is_open(view: SidePanelView) -> bool {
    !matches!(view, SidePanelView::None)
}

fn open_side_drawer(
    side_panel_view: RwSignal<SidePanelView>,
    last_panel: StoredValue<SidePanelView>,
    mobile_nav_open: RwSignal<bool>,
) {
    mobile_nav_open.set(false);
    let last = last_panel.get_value();
    if side_panel_is_open(last) {
        side_panel_view.set(last);
    } else {
        side_panel_view.set(SidePanelView::Workspace);
    }
}

fn close_side_drawer(
    side_panel_view: RwSignal<SidePanelView>,
    last_panel: StoredValue<SidePanelView>,
) {
    let cur = side_panel_view.get_untracked();
    if side_panel_is_open(cur) {
        last_panel.set_value(cur);
    }
    side_panel_view.set(SidePanelView::None);
}

fn maybe_complete_swipe(
    origin: &Cell<Option<TouchOrigin>>,
    narrow: RwSignal<bool>,
    side_panel_view: RwSignal<SidePanelView>,
    last_panel: StoredValue<SidePanelView>,
    mobile_nav_open: RwSignal<bool>,
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
        if dx >= SWIPE_THRESHOLD_PX {
            close_side_drawer(side_panel_view, last_panel);
        }
    } else if dx <= -SWIPE_THRESHOLD_PX {
        open_side_drawer(side_panel_view, last_panel, mobile_nav_open);
    }
}

fn viewport_width_px() -> f64 {
    web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

fn touch_origin_for_pointer(
    ev: &web_sys::PointerEvent,
    side_panel_view: RwSignal<SidePanelView>,
) -> Option<TouchOrigin> {
    let x = f64::from(ev.client_x());
    let y = f64::from(ev.client_y());
    if side_panel_is_open(side_panel_view.get_untracked()) {
        return Some(TouchOrigin {
            x,
            y,
            closing: true,
        });
    }
    let w = viewport_width_px();
    if w > 0.0 && x >= w - EDGE_ZONE_PX {
        Some(TouchOrigin {
            x,
            y,
            closing: false,
        })
    } else {
        None
    }
}

fn wire_remember_last_side_panel(
    side_panel_view: RwSignal<SidePanelView>,
    last_panel: StoredValue<SidePanelView>,
) {
    Effect::new(move |_| {
        let v = side_panel_view.get();
        if side_panel_is_open(v) {
            last_panel.set_value(v);
        }
    });
}

fn wire_side_nav_mutual_exclusion(
    narrow: RwSignal<bool>,
    side_panel_view: RwSignal<SidePanelView>,
    mobile_nav_open: RwSignal<bool>,
) {
    Effect::new(move |_| {
        if !narrow.get() {
            return;
        }
        if mobile_nav_open.get() && side_panel_is_open(side_panel_view.get_untracked()) {
            side_panel_view.set(SidePanelView::None);
        }
    });
    Effect::new(move |_| {
        if !narrow.get() {
            return;
        }
        if side_panel_is_open(side_panel_view.get()) && mobile_nav_open.get_untracked() {
            mobile_nav_open.set(false);
        }
    });
}

fn attach_side_edge_pointer_listeners(
    narrow: RwSignal<bool>,
    side_panel_view: RwSignal<SidePanelView>,
    mobile_nav_open: RwSignal<bool>,
    last_panel: StoredValue<SidePanelView>,
) {
    Effect::new(move |_| {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Some(doc) = window.document() else {
            return;
        };

        let origin: Rc<Cell<Option<TouchOrigin>>> = Rc::new(Cell::new(None));

        let origin_down = Rc::clone(&origin);
        let on_down = Closure::<dyn FnMut(web_sys::PointerEvent)>::new({
            let doc = doc.clone();
            move |ev: web_sys::PointerEvent| {
                if !narrow.get_untracked()
                    || (ev.pointer_type() == "mouse" && ev.button() != 0)
                    || overlay_blocks_swipe(&doc)
                    || target_is_text_field(&ev)
                    || mobile_nav_open.get_untracked()
                {
                    origin_down.set(None);
                    return;
                }
                origin_down.set(touch_origin_for_pointer(&ev, side_panel_view));
            }
        });

        let origin_up = Rc::clone(&origin);
        let on_up =
            Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |ev: web_sys::PointerEvent| {
                maybe_complete_swipe(
                    &origin_up,
                    narrow,
                    side_panel_view,
                    last_panel,
                    mobile_nav_open,
                    &ev,
                );
            });

        let origin_cancel = Rc::clone(&origin);
        let on_cancel =
            Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |ev: web_sys::PointerEvent| {
                maybe_complete_swipe(
                    &origin_cancel,
                    narrow,
                    side_panel_view,
                    last_panel,
                    mobile_nav_open,
                    &ev,
                );
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

/// 窄屏下挂接 pointer 边缘滑动：左划开右侧工作区抽屉，右划关。
pub fn wire_mobile_side_edge_swipe(sig: WireMobileSideEdgeSwipeSignals) {
    let last_panel = StoredValue::new(SidePanelView::Workspace);
    wire_remember_last_side_panel(sig.side_panel_view, last_panel);
    wire_side_nav_mutual_exclusion(
        sig.is_narrow_viewport,
        sig.side_panel_view,
        sig.mobile_nav_open,
    );
    attach_side_edge_pointer_listeners(
        sig.is_narrow_viewport,
        sig.side_panel_view,
        sig.mobile_nav_open,
        last_panel,
    );
}
