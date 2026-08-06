//! 消息回合右键 / 长按菜单（替代气泡下方常驻操作条）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gloo_timers::callback::Timeout;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::tui_actions_bar::{
    TuiTurnActionHandlers, dispatch_tui_turn_action, turn_menu_action_keys,
};
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::{self, Locale};
use crate::session_ops::clamp_session_ctx_menu_pos;
use crate::storage::StoredMessage;

const LONG_PRESS_MS: u32 = 480;
const MOVE_CANCEL_PX: f64 = 14.0;
const SUPPRESS_CLICK_MS: u32 = 700;

/// 打开中的消息操作菜单锚点。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MessageTurnMenuAnchor {
    pub x: f64,
    pub y: f64,
    pub message_id: String,
    pub msg_idx: usize,
    pub action_keys: Vec<&'static str>,
}

type TimerSlot = Rc<RefCell<Option<Timeout>>>;
type SuppressFlag = Rc<Cell<bool>>;
type PressOrigin = Rc<Cell<Option<(f64, f64)>>>;

fn menu_label(locale: Locale, action: &str) -> &'static str {
    match action {
        "copy" => i18n::msg_menu_copy(locale),
        "regen" => i18n::msg_menu_regen(locale),
        "branch" => i18n::msg_menu_branch(locale),
        "retry" => i18n::msg_menu_retry(locale),
        _ => "?",
    }
}

fn menu_title(locale: Locale, action: &str) -> &'static str {
    match action {
        "copy" => i18n::msg_copy_title(locale),
        "regen" => i18n::msg_regen_title(locale),
        "branch" => i18n::msg_branch_title(locale),
        "retry" => i18n::msg_retry_title(locale),
        _ => menu_label(locale, action),
    }
}

fn action_is_busy_gated(action: &str) -> bool {
    matches!(action, "regen" | "branch" | "retry")
}

fn action_is_danger(action: &str) -> bool {
    matches!(action, "branch" | "regen")
}

fn lookup_message(chat: ChatSessionSignals, message_id: &str) -> Option<(usize, StoredMessage)> {
    chat.sessions.with(|list| {
        let aid = chat.active_id.get_untracked();
        let session = list.iter().find(|s| s.id == aid)?;
        session
            .messages
            .iter()
            .enumerate()
            .find(|(_, m)| m.id == message_id)
            .map(|(idx, m)| (idx, m.clone()))
    })
}

fn open_menu_at(
    menu: RwSignal<Option<MessageTurnMenuAnchor>>,
    chat: ChatSessionSignals,
    message_id: String,
    client_x: i32,
    client_y: i32,
) {
    let Some((idx, message)) = lookup_message(chat, &message_id) else {
        return;
    };
    let action_keys = turn_menu_action_keys(&message);
    if action_keys.is_empty() {
        return;
    }
    let (x, y) = clamp_session_ctx_menu_pos(client_x, client_y);
    menu.set(Some(MessageTurnMenuAnchor {
        x,
        y,
        message_id,
        msg_idx: idx,
        action_keys,
    }));
}

fn wrap_from_event_target(target: &web_sys::EventTarget) -> Option<web_sys::HtmlElement> {
    let el = target.dyn_ref::<web_sys::Element>()?;
    el.closest(".chat-tui-turn-wrap")
        .ok()
        .flatten()?
        .dyn_into::<web_sys::HtmlElement>()
        .ok()
}

fn message_id_from_wrap(wrap: &web_sys::HtmlElement) -> Option<String> {
    wrap.get_attribute("data-tui-wrap-id")
}

/// 绑定在 transcript 上的右键 / 长按处理器。
pub(crate) struct MessageTurnPressHandlers {
    pub on_contextmenu: Rc<dyn Fn(web_sys::MouseEvent)>,
    pub on_pointerdown: Rc<dyn Fn(web_sys::PointerEvent)>,
    pub on_pointermove: Rc<dyn Fn(web_sys::PointerEvent)>,
    pub on_pointer_end: Rc<dyn Fn()>,
    pub try_consume_suppress_click: Rc<dyn Fn() -> bool>,
}

#[must_use]
pub(crate) fn build_message_turn_press_handlers(
    chat: ChatSessionSignals,
    menu: RwSignal<Option<MessageTurnMenuAnchor>>,
) -> MessageTurnPressHandlers {
    let long_press_timer: TimerSlot = Rc::new(RefCell::new(None));
    let suppress_clear_timer: TimerSlot = Rc::new(RefCell::new(None));
    let press_origin: PressOrigin = Rc::new(Cell::new(None));
    let suppress_click: SuppressFlag = Rc::new(Cell::new(false));

    let arm_suppress = {
        let suppress_click = Rc::clone(&suppress_click);
        let suppress_clear_timer = Rc::clone(&suppress_clear_timer);
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
        }) as Rc<dyn Fn()>
    };

    let clear_long_press = {
        let long_press_timer = Rc::clone(&long_press_timer);
        let press_origin = Rc::clone(&press_origin);
        Rc::new(move || {
            if let Some(t) = long_press_timer.borrow_mut().take() {
                t.cancel();
            }
            press_origin.set(None);
        }) as Rc<dyn Fn()>
    };

    let on_contextmenu = {
        let arm_suppress = Rc::clone(&arm_suppress);
        let clear_long_press = Rc::clone(&clear_long_press);
        Rc::new(move |ev: web_sys::MouseEvent| {
            let Some(wrap) = ev.target().as_ref().and_then(wrap_from_event_target) else {
                return;
            };
            let Some(message_id) = message_id_from_wrap(&wrap) else {
                return;
            };
            // 选区非空时保留浏览器右键（复制选中文本等）
            if web_sys::window()
                .and_then(|w| w.get_selection().ok().flatten())
                .is_some_and(|s| {
                    !s.is_collapsed() && !String::from(s.to_string()).trim().is_empty()
                })
            {
                return;
            }
            ev.prevent_default();
            clear_long_press();
            arm_suppress();
            open_menu_at(menu, chat, message_id, ev.client_x(), ev.client_y());
        }) as Rc<dyn Fn(web_sys::MouseEvent)>
    };

    let on_pointerdown = {
        let long_press_timer = Rc::clone(&long_press_timer);
        let press_origin = Rc::clone(&press_origin);
        let arm_suppress = Rc::clone(&arm_suppress);
        Rc::new(move |ev: web_sys::PointerEvent| {
            if ev.pointer_type() == "mouse" {
                return;
            }
            let Some(wrap) = ev.target().as_ref().and_then(wrap_from_event_target) else {
                return;
            };
            let Some(message_id) = message_id_from_wrap(&wrap) else {
                return;
            };
            if let Some(t) = long_press_timer.borrow_mut().take() {
                t.cancel();
            }
            press_origin.set(Some((f64::from(ev.client_x()), f64::from(ev.client_y()))));
            let long_press_timer_c = Rc::clone(&long_press_timer);
            let arm_suppress = Rc::clone(&arm_suppress);
            let x = ev.client_x();
            let y = ev.client_y();
            *long_press_timer.borrow_mut() = Some(Timeout::new(LONG_PRESS_MS, move || {
                long_press_timer_c.borrow_mut().take();
                arm_suppress();
                open_menu_at(menu, chat, message_id, x, y);
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
            let dx = f64::from(ev.client_x()) - ox;
            let dy = f64::from(ev.client_y()) - oy;
            if dx * dx + dy * dy > MOVE_CANCEL_PX * MOVE_CANCEL_PX {
                clear_long_press();
            }
        }) as Rc<dyn Fn(web_sys::PointerEvent)>
    };

    let try_consume_suppress_click = {
        let suppress_click = Rc::clone(&suppress_click);
        let suppress_clear_timer = Rc::clone(&suppress_clear_timer);
        Rc::new(move || -> bool {
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

    MessageTurnPressHandlers {
        on_contextmenu,
        on_pointerdown,
        on_pointermove,
        on_pointer_end: clear_long_press,
        try_consume_suppress_click,
    }
}

#[component]
pub(crate) fn MessageTurnContextMenuLayer(
    locale: RwSignal<Locale>,
    menu: RwSignal<Option<MessageTurnMenuAnchor>>,
    action_handlers: TuiTurnActionHandlers,
) -> impl IntoView {
    view! {
        <Show when=move || menu.get().is_some()>
            <div class="session-ctx-layer message-turn-ctx-layer" data-testid="message-turn-ctx-menu">
                <div
                    class="session-ctx-backdrop"
                    on:click=move |_| menu.set(None)
                ></div>
                <div
                    class="session-ctx-menu"
                    role="menu"
                    style=move || {
                        menu.get()
                            .map(|a| format!("left:{}px;top:{}px", a.x, a.y))
                            .unwrap_or_default()
                    }
                >
                    {move || {
                        let Some(anchor) = menu.get() else {
                            return view! { <span></span> }.into_any();
                        };
                        let busy = action_handlers.stream_turn_busy_ui.get();
                        let loc = locale.get();
                        anchor
                            .action_keys
                            .iter()
                            .copied()
                            .map(|action| {
                                let message_id = anchor.message_id.clone();
                                let msg_idx = anchor.msg_idx;
                                let disabled = action_is_busy_gated(action) && busy;
                                let danger = action_is_danger(action);
                                let label = menu_label(loc, action);
                                let title = menu_title(loc, action);
                                let test_id = format!("message-turn-ctx-{action}");
                                view! {
                                    <button
                                        type="button"
                                        class="session-ctx-item"
                                        class:session-ctx-item-danger=danger
                                        role="menuitem"
                                        prop:disabled=disabled
                                        prop:title=title
                                        data-testid=test_id
                                        on:click=move |_| {
                                            if action_is_busy_gated(action)
                                                && action_handlers.stream_turn_busy_ui.get_untracked()
                                            {
                                                return;
                                            }
                                            menu.set(None);
                                            let _ = dispatch_tui_turn_action(
                                                action_handlers,
                                                action,
                                                &message_id,
                                                msg_idx,
                                            );
                                        }
                                    >
                                        {label}
                                    </button>
                                }
                                .into_any()
                            })
                            .collect_view()
                            .into_any()
                    }}
                </div>
            </div>
        </Show>
    }
}
