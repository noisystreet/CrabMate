//! 工作区树行触摸长按 / contextmenu 后吞 click，避免菜单被立刻关掉或误触展开/插入。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gloo_timers::callback::Timeout;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::workspace_context_menu::WorkspaceContextAnchor;

const LONG_PRESS_MS: u32 = 480;
const MOVE_CANCEL_PX: f64 = 14.0;
const SUPPRESS_CLICK_MS: u32 = 700;

type TimerSlot = Rc<RefCell<Option<Timeout>>>;
type SuppressFlag = Rc<Cell<bool>>;
type PressOrigin = Rc<Cell<Option<(f64, f64)>>>;
type ArmSuppress = Rc<dyn Fn()>;
type ClearLongPress = Rc<dyn Fn()>;

#[derive(Clone)]
pub(crate) struct WorkspaceRowPressTarget {
    pub target_rel: Option<String>,
    pub target_is_dir: bool,
    pub parent_rel: String,
}

pub(crate) struct WorkspaceRowPressHandlers {
    pub on_contextmenu: Rc<dyn Fn(web_sys::MouseEvent)>,
    pub on_pointerdown: Rc<dyn Fn(web_sys::PointerEvent)>,
    pub on_pointermove: Rc<dyn Fn(web_sys::PointerEvent)>,
    pub on_pointer_end: ClearLongPress,
    pub try_consume_suppress_click: Rc<dyn Fn() -> bool>,
}

pub(crate) fn open_workspace_context_menu_at(
    workspace_context_menu: RwSignal<Option<WorkspaceContextAnchor>>,
    x: f64,
    y: f64,
    target: WorkspaceRowPressTarget,
) {
    workspace_context_menu.set(Some(WorkspaceContextAnchor {
        x,
        y,
        target_rel: target.target_rel,
        target_is_dir: target.target_is_dir,
        parent_rel: target.parent_rel,
    }));
}

pub(crate) fn build_workspace_row_press_handlers(
    workspace_context_menu: RwSignal<Option<WorkspaceContextAnchor>>,
    target: WorkspaceRowPressTarget,
) -> WorkspaceRowPressHandlers {
    let long_press_timer: TimerSlot = Rc::new(RefCell::new(None));
    let suppress_clear_timer: TimerSlot = Rc::new(RefCell::new(None));
    let press_origin: PressOrigin = Rc::new(Cell::new(None));
    let suppress_click: SuppressFlag = Rc::new(Cell::new(false));

    let arm_suppress_click = make_arm_suppress_click(&suppress_click, &suppress_clear_timer);
    let clear_long_press = make_clear_long_press(&long_press_timer, &press_origin);

    let on_contextmenu = {
        let target = target.clone();
        let arm_suppress_click = Rc::clone(&arm_suppress_click);
        let clear_long_press = Rc::clone(&clear_long_press);
        Rc::new(move |ev: web_sys::MouseEvent| {
            ev.prevent_default();
            ev.stop_propagation();
            clear_long_press();
            arm_suppress_click();
            open_workspace_context_menu_at(
                workspace_context_menu,
                f64::from(ev.client_x()),
                f64::from(ev.client_y()),
                target.clone(),
            );
        }) as Rc<dyn Fn(web_sys::MouseEvent)>
    };

    let on_pointerdown = {
        let target = target.clone();
        let arm_suppress_click = Rc::clone(&arm_suppress_click);
        let long_press_timer = Rc::clone(&long_press_timer);
        let press_origin = Rc::clone(&press_origin);
        Rc::new(move |ev: web_sys::PointerEvent| {
            if ev.pointer_type() == "mouse" || ev.button() != 0 {
                return;
            }
            let x = ev.client_x();
            let y = ev.client_y();
            press_origin.set(Some((f64::from(x), f64::from(y))));
            if let Some(t) = long_press_timer.borrow_mut().take() {
                t.cancel();
            }
            let target = target.clone();
            let arm_suppress_click = Rc::clone(&arm_suppress_click);
            let long_press_timer_c = Rc::clone(&long_press_timer);
            *long_press_timer.borrow_mut() = Some(Timeout::new(LONG_PRESS_MS, move || {
                long_press_timer_c.borrow_mut().take();
                arm_suppress_click();
                open_workspace_context_menu_at(
                    workspace_context_menu,
                    f64::from(x),
                    f64::from(y),
                    target,
                );
            }));
        }) as Rc<dyn Fn(web_sys::PointerEvent)>
    };

    let on_pointermove = {
        let press_origin = Rc::clone(&press_origin);
        let clear_long_press = Rc::clone(&clear_long_press);
        Rc::new(move |ev: web_sys::PointerEvent| {
            let Some((ox, oy)) = press_origin.get() else {
                return;
            };
            let dx = (f64::from(ev.client_x()) - ox).abs();
            let dy = (f64::from(ev.client_y()) - oy).abs();
            if dx > MOVE_CANCEL_PX || dy > MOVE_CANCEL_PX {
                clear_long_press();
            }
        }) as Rc<dyn Fn(web_sys::PointerEvent)>
    };

    let try_consume_suppress_click = {
        let suppress_click = Rc::clone(&suppress_click);
        let suppress_clear_timer = Rc::clone(&suppress_clear_timer);
        Rc::new(move || {
            if !suppress_click.get() {
                return false;
            }
            suppress_click.set(false);
            if let Some(t) = suppress_clear_timer.borrow_mut().take() {
                t.cancel();
            }
            true
        }) as Rc<dyn Fn() -> bool>
    };

    WorkspaceRowPressHandlers {
        on_contextmenu,
        on_pointerdown,
        on_pointermove,
        on_pointer_end: clear_long_press,
        try_consume_suppress_click,
    }
}

/// 空白处长按：仅在目标不是树行 `li` 时打开根目录新建菜单。
pub(crate) fn build_workspace_blank_press_handlers(
    workspace_context_menu: RwSignal<Option<WorkspaceContextAnchor>>,
) -> WorkspaceRowPressHandlers {
    let long_press_timer: TimerSlot = Rc::new(RefCell::new(None));
    let suppress_clear_timer: TimerSlot = Rc::new(RefCell::new(None));
    let press_origin: PressOrigin = Rc::new(Cell::new(None));
    let suppress_click: SuppressFlag = Rc::new(Cell::new(false));
    let arm_suppress_click = make_arm_suppress_click(&suppress_click, &suppress_clear_timer);
    let clear_long_press = make_clear_long_press(&long_press_timer, &press_origin);
    let blank_target = WorkspaceRowPressTarget {
        target_rel: None,
        target_is_dir: false,
        parent_rel: String::new(),
    };

    let on_contextmenu = {
        let arm_suppress_click = Rc::clone(&arm_suppress_click);
        let clear_long_press = Rc::clone(&clear_long_press);
        let blank_target = blank_target.clone();
        Rc::new(move |ev: web_sys::MouseEvent| {
            if blank_press_should_ignore(ev.target()) {
                return;
            }
            ev.prevent_default();
            clear_long_press();
            arm_suppress_click();
            open_workspace_context_menu_at(
                workspace_context_menu,
                f64::from(ev.client_x()),
                f64::from(ev.client_y()),
                blank_target.clone(),
            );
        }) as Rc<dyn Fn(web_sys::MouseEvent)>
    };

    let on_pointerdown = {
        let arm_suppress_click = Rc::clone(&arm_suppress_click);
        let long_press_timer = Rc::clone(&long_press_timer);
        let press_origin = Rc::clone(&press_origin);
        let blank_target = blank_target.clone();
        Rc::new(move |ev: web_sys::PointerEvent| {
            if ev.pointer_type() == "mouse" || ev.button() != 0 {
                return;
            }
            if blank_press_should_ignore(ev.target()) {
                return;
            }
            let x = ev.client_x();
            let y = ev.client_y();
            press_origin.set(Some((f64::from(x), f64::from(y))));
            if let Some(t) = long_press_timer.borrow_mut().take() {
                t.cancel();
            }
            let blank_target = blank_target.clone();
            let arm_suppress_click = Rc::clone(&arm_suppress_click);
            let long_press_timer_c = Rc::clone(&long_press_timer);
            *long_press_timer.borrow_mut() = Some(Timeout::new(LONG_PRESS_MS, move || {
                long_press_timer_c.borrow_mut().take();
                arm_suppress_click();
                open_workspace_context_menu_at(
                    workspace_context_menu,
                    f64::from(x),
                    f64::from(y),
                    blank_target,
                );
            }));
        }) as Rc<dyn Fn(web_sys::PointerEvent)>
    };

    let on_pointermove = {
        let press_origin = Rc::clone(&press_origin);
        let clear_long_press = Rc::clone(&clear_long_press);
        Rc::new(move |ev: web_sys::PointerEvent| {
            let Some((ox, oy)) = press_origin.get() else {
                return;
            };
            let dx = (f64::from(ev.client_x()) - ox).abs();
            let dy = (f64::from(ev.client_y()) - oy).abs();
            if dx > MOVE_CANCEL_PX || dy > MOVE_CANCEL_PX {
                clear_long_press();
            }
        }) as Rc<dyn Fn(web_sys::PointerEvent)>
    };

    let try_consume_suppress_click = {
        let suppress_click = Rc::clone(&suppress_click);
        let suppress_clear_timer = Rc::clone(&suppress_clear_timer);
        Rc::new(move || {
            if !suppress_click.get() {
                return false;
            }
            suppress_click.set(false);
            if let Some(t) = suppress_clear_timer.borrow_mut().take() {
                t.cancel();
            }
            true
        }) as Rc<dyn Fn() -> bool>
    };

    WorkspaceRowPressHandlers {
        on_contextmenu,
        on_pointerdown,
        on_pointermove,
        on_pointer_end: clear_long_press,
        try_consume_suppress_click,
    }
}

fn make_arm_suppress_click(
    suppress_click: &SuppressFlag,
    suppress_clear_timer: &TimerSlot,
) -> ArmSuppress {
    let suppress_click = Rc::clone(suppress_click);
    let suppress_clear_timer = Rc::clone(suppress_clear_timer);
    Rc::new(move || {
        suppress_click.set(true);
        if let Some(t) = suppress_clear_timer.borrow_mut().take() {
            t.cancel();
        }
        let suppress_click = Rc::clone(&suppress_click);
        let suppress_clear_timer_c = Rc::clone(&suppress_clear_timer);
        *suppress_clear_timer.borrow_mut() = Some(Timeout::new(SUPPRESS_CLICK_MS, move || {
            suppress_clear_timer_c.borrow_mut().take();
            suppress_click.set(false);
        }));
    })
}

fn make_clear_long_press(
    long_press_timer: &TimerSlot,
    press_origin: &PressOrigin,
) -> ClearLongPress {
    let long_press_timer = Rc::clone(long_press_timer);
    let press_origin = Rc::clone(press_origin);
    Rc::new(move || {
        if let Some(t) = long_press_timer.borrow_mut().take() {
            t.cancel();
        }
        press_origin.set(None);
    })
}

fn blank_press_should_ignore(target: Option<web_sys::EventTarget>) -> bool {
    let Some(t) = target else {
        return false;
    };
    let Some(el) = t.dyn_ref::<web_sys::Element>() else {
        return false;
    };
    // 文件行 / 目录头由行级 handler 处理；勿用裸 `li`（父级 dir 节点会吞掉嵌套区）
    if el
        .closest("li.file, .workspace-dir-head")
        .ok()
        .flatten()
        .is_some()
    {
        return true;
    }
    if el
        .closest(".shell-topbar-workspace")
        .ok()
        .flatten()
        .is_some()
    {
        return true;
    }
    // 菜单层挂在 shell 内且 fixed 全屏，pointer 会冒泡到空白长按
    el.closest(".session-ctx-layer, .workspace-ctx-layer")
        .ok()
        .flatten()
        .is_some()
}
