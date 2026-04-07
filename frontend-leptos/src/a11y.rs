//! 无障碍辅助：模态内焦点、Tab 循环。

use wasm_bindgen::JsCast;

/// 在容器内聚焦第一个可 Tab 停驻的子节点（若无则聚焦容器自身）。
pub fn focus_first_in_modal_container(container: &web_sys::Element) {
    let Ok(list) = container.query_selector_all(
        "button:not([disabled]), [href], input:not([disabled]):not([type='hidden']), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
    ) else {
        let _ = container
            .dyn_ref::<web_sys::HtmlElement>()
            .map(|h| h.focus());
        return;
    };
    let n = list.length();
    for i in 0..n {
        if let Some(node) = list.item(i) {
            if let Ok(el) = node.dyn_into::<web_sys::HtmlElement>() {
                // 跳过 `aria-hidden` 祖先内的节点
                if is_hidden_from_a11y_tree(&el) {
                    continue;
                }
                let _ = el.focus();
                return;
            }
        }
    }
    let _ = container
        .dyn_ref::<web_sys::HtmlElement>()
        .map(|h| h.focus());
}

fn is_hidden_from_a11y_tree(el: &web_sys::HtmlElement) -> bool {
    let mut cur: Option<web_sys::Element> = Some(el.clone().unchecked_into());
    while let Some(node) = cur {
        if node.tag_name().eq_ignore_ascii_case("BODY") {
            break;
        }
        if node.get_attribute("aria-hidden").as_deref() == Some("true") {
            return true;
        }
        cur = node.parent_element();
    }
    false
}

/// `Tab` / `Shift+Tab` 时将焦点限制在 `container` 内。
pub fn trap_tab_in_container(ev: &web_sys::KeyboardEvent, container: &web_sys::Element) {
    if ev.key() != "Tab" {
        return;
    }
    let Ok(list) = container.query_selector_all(
        "button:not([disabled]), [href], input:not([disabled]):not([type='hidden']), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])",
    ) else {
        return;
    };
    let mut els: Vec<web_sys::HtmlElement> = Vec::new();
    let n = list.length();
    for i in 0..n {
        if let Some(node) = list.item(i) {
            if let Ok(el) = node.dyn_into::<web_sys::HtmlElement>() {
                if !is_hidden_from_a11y_tree(&el) {
                    els.push(el);
                }
            }
        }
    }
    if els.is_empty() {
        return;
    }

    let doc = leptos_dom::helpers::document();
    let Some(active) = doc.active_element() else {
        return;
    };
    let Ok(active_el) = active.dyn_into::<web_sys::HtmlElement>() else {
        return;
    };

    let mut idx_opt = None;
    for (i, el) in els.iter().enumerate() {
        if active_el.is_same_node(Some(el.as_ref())) {
            idx_opt = Some(i);
            break;
        }
    }
    let shift = ev.shift_key();
    ev.prevent_default();

    match idx_opt {
        None => {
            let _ = els[0].focus();
        }
        Some(idx) if !shift => {
            if idx + 1 < els.len() {
                let _ = els[idx + 1].focus();
            } else {
                let _ = els[0].focus();
            }
        }
        Some(idx) => {
            if idx > 0 {
                let _ = els[idx - 1].focus();
            } else {
                let _ = els.last().expect("non-empty").focus();
            }
        }
    }
}
