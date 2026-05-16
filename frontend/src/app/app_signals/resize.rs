//! 侧栏宽度拖拽会话状态。

use std::cell::RefCell;
use std::rc::Rc;

use leptos::prelude::*;
use leptos_dom::helpers::WindowListenerHandle;

#[derive(Clone)]
pub struct ResizeSignals {
    pub side_resize_session: Rc<RefCell<Option<(f64, f64)>>>,
    pub side_resize_handles: Rc<RefCell<Option<(WindowListenerHandle, WindowListenerHandle)>>>,
    pub side_resize_dragging: RwSignal<bool>,
}

impl ResizeSignals {
    pub fn new() -> Self {
        Self {
            side_resize_session: Rc::new(RefCell::new(None)),
            side_resize_handles: Rc::new(RefCell::new(None)),
            side_resize_dragging: RwSignal::new(false),
        }
    }
}

impl Default for ResizeSignals {
    fn default() -> Self {
        Self::new()
    }
}
