use std::cell::Cell;
use std::rc::Rc;

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::debounce_schedule;

pub(super) const SIDEBAR_SESSION_FILTER_DEBOUNCE_MS: u32 = 250;
pub(super) const GLOBAL_MESSAGE_SEARCH_DEBOUNCE_MS: u32 = 250;

pub(super) fn debounce_signal_to_effect(
    source: RwSignal<String>,
    target: RwSignal<String>,
    delay_ms: u32,
) {
    let debounce_seq: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    Effect::new({
        let debounce_seq = Rc::clone(&debounce_seq);
        move |_| {
            let v = source.get();
            let id = debounce_seq.get().wrapping_add(1);
            debounce_seq.set(id);
            let seq = Rc::clone(&debounce_seq);
            spawn_local(async move {
                TimeoutFuture::new(delay_ms).await;
                if debounce_schedule::debounce_should_apply(id, seq.get()) {
                    target.set(v);
                }
            });
        }
    });
}

pub(super) fn rail_context_menu_target_is_session_row_or_hit(ev: &web_sys::MouseEvent) -> bool {
    let Some(t) = ev.target() else {
        return false;
    };
    let Ok(el) = t.dyn_into::<web_sys::Element>() else {
        return false;
    };
    el.closest(".nav-session-item").ok().flatten().is_some()
        || el.closest(".nav-search-hit").ok().flatten().is_some()
}
