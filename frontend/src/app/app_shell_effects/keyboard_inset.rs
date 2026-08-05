//! `visualViewport` 键盘避让：把键盘占用高度写入 `--vv-keyboard-inset`，供窄屏 composer 抬高。

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

fn apply_keyboard_inset_css() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(doc) = window.document() else {
        return;
    };
    let Some(root) = doc.document_element() else {
        return;
    };
    let Ok(root) = root.dyn_into::<web_sys::HtmlElement>() else {
        return;
    };

    let inset_px = (|| -> Option<f64> {
        let vv = window.visual_viewport()?;
        let layout_h = window.inner_height().ok()?.as_f64()?;
        let vv_h = vv.height();
        let vv_top = vv.offset_top();
        // 键盘弹出时 visualViewport 变矮；offsetTop 可能上移。
        Some((layout_h - vv_h - vv_top).max(0.0))
    })()
    .unwrap_or(0.0);

    let _ = root
        .style()
        .set_property("--vv-keyboard-inset", &format!("{inset_px:.0}px"));
}

/// 订阅 `visualViewport` 的 resize/scroll，维护 `--vv-keyboard-inset`。
pub fn wire_visual_viewport_keyboard_inset() {
    Effect::new(move |_| {
        let Some(window) = web_sys::window() else {
            return;
        };
        apply_keyboard_inset_css();

        let Some(vv) = window.visual_viewport() else {
            return;
        };

        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            apply_keyboard_inset_css();
        });
        let _ = vv.add_event_listener_with_callback("resize", cb.as_ref().unchecked_ref());
        let _ = vv.add_event_listener_with_callback("scroll", cb.as_ref().unchecked_ref());
        // 部分 WebView 只在 window resize 上反映键盘
        let _ = window.add_event_listener_with_callback("resize", cb.as_ref().unchecked_ref());
        cb.forget();
    });
}
