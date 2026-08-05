//! `visualViewport`：虚拟键盘顶起时补偿 composer 底部间距。

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

pub fn wire_visual_viewport_keyboard_inset() {
    Effect::new(|_| {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Ok(vv_val) = js_sys::Reflect::get(
            window.as_ref(),
            &wasm_bindgen::JsValue::from_str("visualViewport"),
        ) else {
            return;
        };
        if vv_val.is_null() || vv_val.is_undefined() {
            return;
        };
        let Ok(vv) = vv_val.clone().dyn_into::<js_sys::Object>() else {
            return;
        };

        let apply = {
            let window = window.clone();
            move |vv: &js_sys::Object| {
                let inner_h = js_sys::Reflect::get(vv, &wasm_bindgen::JsValue::from_str("height"))
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let offset_top =
                    js_sys::Reflect::get(vv, &wasm_bindgen::JsValue::from_str("offsetTop"))
                        .ok()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                let layout_h = window
                    .inner_height()
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let keyboard = (layout_h - inner_h - offset_top).max(0.0);
                if let Some(doc) = window.document() {
                    if let Some(root) = doc.document_element() {
                        let px = format!("{keyboard}px");
                        let _ = root
                            .unchecked_ref::<web_sys::HtmlElement>()
                            .style()
                            .set_property("--keyboard-inset-bottom", px.as_str());
                    }
                }
            }
        };

        apply(&vv);

        let vv_for_cb = vv_val.clone();
        let Ok(target) = vv_for_cb.clone().dyn_into::<web_sys::EventTarget>() else {
            return;
        };
        let window_for_cb = window.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            if let Ok(vv_obj) = vv_for_cb.clone().dyn_into::<js_sys::Object>() {
                let inner_h =
                    js_sys::Reflect::get(&vv_obj, &wasm_bindgen::JsValue::from_str("height"))
                        .ok()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                let offset_top =
                    js_sys::Reflect::get(&vv_obj, &wasm_bindgen::JsValue::from_str("offsetTop"))
                        .ok()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                let layout_h = window_for_cb
                    .inner_height()
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let keyboard = (layout_h - inner_h - offset_top).max(0.0);
                if let Some(doc) = window_for_cb.document() {
                    if let Some(root) = doc.document_element() {
                        let px = format!("{keyboard}px");
                        let _ = root
                            .unchecked_ref::<web_sys::HtmlElement>()
                            .style()
                            .set_property("--keyboard-inset-bottom", px.as_str());
                    }
                }
            }
        });
        let _ = target.add_event_listener_with_callback("resize", cb.as_ref().unchecked_ref());
        let _ = target.add_event_listener_with_callback("scroll", cb.as_ref().unchecked_ref());
        cb.forget();
    });
}
