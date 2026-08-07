//! `visualViewport` 键盘避让：把键盘占用高度写入 `--vv-keyboard-inset`，供窄屏 composer 抬高。

use leptos::prelude::*;
use leptos::task::spawn_local;
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
        // 若 Activity 已 adjustResize，innerHeight 与 vv 同步缩小 → inset≈0（由窗口变矮抬起 composer）。
        Some((layout_h - vv_h - vv_top).max(0.0))
    })()
    .unwrap_or(0.0);

    let _ = root
        .style()
        .set_property("--vv-keyboard-inset", &format!("{inset_px:.0}px"));
}

/// 立即重算 `--vv-keyboard-inset`（供 focus 等早于 `visualViewport` 事件的路径调用）。
pub(crate) fn refresh_keyboard_inset() {
    apply_keyboard_inset_css();
}

fn root_has_attr(name: &str) -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let Some(doc) = window.document() else {
        return false;
    };
    let Some(root) = doc.document_element() else {
        return false;
    };
    root.has_attribute(name)
}

fn should_keep_composer_visible() -> bool {
    // 窄屏媒体查询，或 Android 远程壳（横屏可能 >768px）。
    root_has_attr("data-narrow-viewport") || root_has_attr("data-cm-mobile-shell")
}

fn composer_bar_element(ta: &web_sys::HtmlTextAreaElement) -> web_sys::HtmlElement {
    let mut node: Option<web_sys::Node> = Some(ta.clone().into());
    while let Some(n) = node {
        if let Ok(el) = n.clone().dyn_into::<web_sys::HtmlElement>()
            && el.class_list().contains("composer-ds")
        {
            return el;
        }
        node = n.parent_node();
    }
    ta.clone().into()
}

fn scroll_composer_into_view(ta: &web_sys::HtmlTextAreaElement) {
    let bar = composer_bar_element(ta);
    // false：尽量贴齐视口底边，避免软键盘动画期间把输入条滚出可见区上方。
    bar.scroll_into_view_with_bool(false);
}

/// 窄屏 / 移动壳聚焦聊天输入时：立刻滚动 composer 入视口并重算键盘 inset（软键盘动画期间多次重试）。
pub(crate) fn on_composer_focus_keep_visible(ta: &web_sys::HtmlTextAreaElement) {
    if !should_keep_composer_visible() {
        return;
    }
    scroll_composer_into_view(ta);
    refresh_keyboard_inset();

    let ta = ta.clone();
    spawn_local(async move {
        for delay_ms in [50_u32, 150, 300, 500] {
            gloo_timers::future::TimeoutFuture::new(delay_ms).await;
            scroll_composer_into_view(&ta);
            refresh_keyboard_inset();
        }
    });
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
