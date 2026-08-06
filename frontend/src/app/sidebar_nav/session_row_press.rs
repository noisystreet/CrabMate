//! 会话行触摸长按 / contextmenu 后吞 click，避免菜单被立刻关掉。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gloo_timers::callback::Timeout;
use leptos::prelude::*;

use crate::session_ops::{SessionContextAnchor, clamp_session_ctx_menu_pos};

const LONG_PRESS_MS: u32 = 480;
const MOVE_CANCEL_PX: f64 = 14.0;
/// 打开菜单后吞掉随后可能到达的 click；超时自动清除避免粘住下次点击。
const SUPPRESS_CLICK_MS: u32 = 700;

type TimerSlot = Rc<RefCell<Option<Timeout>>>;
type SuppressFlag = Rc<Cell<bool>>;
type PressOrigin = Rc<Cell<Option<(f64, f64)>>>;
type ArmSuppress = Rc<dyn Fn()>;
type ClearLongPress = Rc<dyn Fn()>;

pub(super) struct SessionRowPressHandlers {
    pub(super) on_contextmenu: Rc<dyn Fn(web_sys::MouseEvent)>,
    pub(super) on_pointerdown: Rc<dyn Fn(web_sys::PointerEvent)>,
    pub(super) on_pointermove: Rc<dyn Fn(web_sys::PointerEvent)>,
    pub(super) on_pointer_end: ClearLongPress,
    pub(super) try_consume_suppress_click: Rc<dyn Fn() -> bool>,
}

pub(super) fn build_session_row_press_handlers(
    session_id: String,
    session_context_menu: RwSignal<Option<SessionContextAnchor>>,
    sidebar_rail_ctx_menu: RwSignal<Option<(f64, f64)>>,
) -> SessionRowPressHandlers {
    let long_press_timer: TimerSlot = Rc::new(RefCell::new(None));
    let suppress_clear_timer: TimerSlot = Rc::new(RefCell::new(None));
    let press_origin: PressOrigin = Rc::new(Cell::new(None));
    let suppress_click: SuppressFlag = Rc::new(Cell::new(false));

    let arm_suppress_click = make_arm_suppress_click(&suppress_click, &suppress_clear_timer);
    let clear_long_press = make_clear_long_press(&long_press_timer, &press_origin);

    let on_contextmenu = make_contextmenu_handler(
        session_id.clone(),
        session_context_menu,
        sidebar_rail_ctx_menu,
        Rc::clone(&arm_suppress_click),
        Rc::clone(&clear_long_press),
    );
    let on_pointerdown = make_pointerdown_handler(
        session_id,
        session_context_menu,
        sidebar_rail_ctx_menu,
        Rc::clone(&long_press_timer),
        Rc::clone(&press_origin),
        arm_suppress_click,
    );
    let on_pointermove =
        make_pointermove_handler(Rc::clone(&press_origin), Rc::clone(&clear_long_press));
    let try_consume_suppress_click =
        make_try_consume_suppress_click(suppress_click, suppress_clear_timer);

    SessionRowPressHandlers {
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

fn open_session_context_menu(
    session_id: String,
    x: i32,
    y: i32,
    session_context_menu: RwSignal<Option<SessionContextAnchor>>,
    sidebar_rail_ctx_menu: RwSignal<Option<(f64, f64)>>,
) {
    sidebar_rail_ctx_menu.set(None);
    let (cx, cy) = clamp_session_ctx_menu_pos(x, y);
    session_context_menu.set(Some(SessionContextAnchor {
        session_id,
        x: cx,
        y: cy,
    }));
}

fn make_contextmenu_handler(
    session_id: String,
    session_context_menu: RwSignal<Option<SessionContextAnchor>>,
    sidebar_rail_ctx_menu: RwSignal<Option<(f64, f64)>>,
    arm_suppress_click: ArmSuppress,
    clear_long_press: ClearLongPress,
) -> Rc<dyn Fn(web_sys::MouseEvent)> {
    Rc::new(move |ev: web_sys::MouseEvent| {
        ev.prevent_default();
        ev.stop_propagation();
        clear_long_press();
        // 触摸长按常先发 contextmenu 再发 click；吞掉随后 click 以免菜单被立刻关掉。
        arm_suppress_click();
        open_session_context_menu(
            session_id.clone(),
            ev.client_x(),
            ev.client_y(),
            session_context_menu,
            sidebar_rail_ctx_menu,
        );
    })
}

fn make_pointerdown_handler(
    session_id: String,
    session_context_menu: RwSignal<Option<SessionContextAnchor>>,
    sidebar_rail_ctx_menu: RwSignal<Option<(f64, f64)>>,
    long_press_timer: TimerSlot,
    press_origin: PressOrigin,
    arm_suppress_click: ArmSuppress,
) -> Rc<dyn Fn(web_sys::PointerEvent)> {
    Rc::new(move |ev: web_sys::PointerEvent| {
        // 仅触摸 / 笔：鼠标仍用右键 contextmenu
        if ev.pointer_type() == "mouse" || ev.button() != 0 {
            return;
        }
        let x = ev.client_x();
        let y = ev.client_y();
        press_origin.set(Some((f64::from(x), f64::from(y))));
        if let Some(t) = long_press_timer.borrow_mut().take() {
            t.cancel();
        }
        let session_id = session_id.clone();
        let arm_suppress_click = Rc::clone(&arm_suppress_click);
        let long_press_timer_c = Rc::clone(&long_press_timer);
        *long_press_timer.borrow_mut() = Some(Timeout::new(LONG_PRESS_MS, move || {
            long_press_timer_c.borrow_mut().take();
            arm_suppress_click();
            open_session_context_menu(
                session_id,
                x,
                y,
                session_context_menu,
                sidebar_rail_ctx_menu,
            );
        }));
    })
}

fn make_pointermove_handler(
    press_origin: PressOrigin,
    clear_long_press: ClearLongPress,
) -> Rc<dyn Fn(web_sys::PointerEvent)> {
    Rc::new(move |ev: web_sys::PointerEvent| {
        let Some((ox, oy)) = press_origin.get() else {
            return;
        };
        let dx = (f64::from(ev.client_x()) - ox).abs();
        let dy = (f64::from(ev.client_y()) - oy).abs();
        if dx > MOVE_CANCEL_PX || dy > MOVE_CANCEL_PX {
            clear_long_press();
        }
    })
}

fn make_try_consume_suppress_click(
    suppress_click: SuppressFlag,
    suppress_clear_timer: TimerSlot,
) -> Rc<dyn Fn() -> bool> {
    Rc::new(move || {
        if !suppress_click.get() {
            return false;
        }
        suppress_click.set(false);
        if let Some(t) = suppress_clear_timer.borrow_mut().take() {
            t.cancel();
        }
        true
    })
}

pub(super) fn session_row_item_class(active: bool, is_pinned: bool, is_starred: bool) -> String {
    let mut c = String::from("nav-session-item");
    if active {
        c.push_str(" is-active");
    }
    if is_pinned {
        c.push_str(" is-pinned");
    }
    if is_starred {
        c.push_str(" is-starred");
    }
    c
}
