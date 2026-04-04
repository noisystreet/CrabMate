//! 工作区侧栏数据刷新与主/侧列拖拽宽度。

use std::cell::RefCell;
use std::rc::Rc;

use leptos::prelude::*;
use leptos_dom::helpers::WindowListenerHandle;
use leptos_dom::helpers::window_event_listener;

use crate::api::{WorkspaceData, fetch_workspace};
use crate::app_prefs::{SidePanelView, clamp_side_width_for_viewport};

pub async fn reload_workspace_panel(
    workspace_loading: RwSignal<bool>,
    workspace_err: RwSignal<Option<String>>,
    workspace_path_draft: RwSignal<String>,
    workspace_data: RwSignal<Option<WorkspaceData>>,
) {
    workspace_loading.set(true);
    match fetch_workspace(None).await {
        Ok(d) => {
            workspace_err.set(None);
            workspace_path_draft.set(d.path.clone());
            workspace_data.set(Some(d));
        }
        Err(e) => {
            workspace_err.set(Some(e));
            workspace_data.set(None);
        }
    }
    workspace_loading.set(false);
}

pub fn begin_side_column_resize(
    ev: web_sys::MouseEvent,
    side_panel_view: RwSignal<SidePanelView>,
    side_width: RwSignal<f64>,
    side_resize_dragging: RwSignal<bool>,
    side_resize_session: Rc<RefCell<Option<(f64, f64)>>>,
    side_resize_handles: Rc<RefCell<Option<(WindowListenerHandle, WindowListenerHandle)>>>,
) {
    if ev.button() != 0 {
        return;
    }
    if matches!(side_panel_view.get_untracked(), SidePanelView::None) {
        return;
    }
    ev.prevent_default();
    if let Some((m, u)) = side_resize_handles.borrow_mut().take() {
        m.remove();
        u.remove();
        *side_resize_session.borrow_mut() = None;
        side_resize_dragging.set(false);
    }

    *side_resize_session.borrow_mut() = Some((ev.client_x() as f64, side_width.get_untracked()));
    side_resize_dragging.set(true);

    let session_m = Rc::clone(&side_resize_session);
    let session_u = Rc::clone(&side_resize_session);
    let handles_slot = Rc::clone(&side_resize_handles);
    let side_w = side_width;
    let drag_sig = side_resize_dragging;

    let hm = window_event_listener(leptos::ev::mousemove, move |e: web_sys::MouseEvent| {
        let borrow = session_m.borrow();
        let Some((sx, sw)) = *borrow else {
            return;
        };
        let cx = e.client_x() as f64;
        side_w.set(clamp_side_width_for_viewport(sw - (cx - sx)));
    });

    let hu = window_event_listener(leptos::ev::mouseup, move |_e: web_sys::MouseEvent| {
        *session_u.borrow_mut() = None;
        drag_sig.set(false);
        if let Some((m, u)) = handles_slot.borrow_mut().take() {
            m.remove();
            u.remove();
        }
    });

    *side_resize_handles.borrow_mut() = Some((hm, hu));
}
