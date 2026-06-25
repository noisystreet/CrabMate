//! 浏览器 / Tauri 通用确认框（桌面 WebView 下 `window.confirm` 常无效）。

use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(inline_js = r#"
export function hasTauriInvokeForConfirm() {
  const direct = globalThis.__TAURI__ && globalThis.__TAURI__.core && globalThis.__TAURI__.core.invoke;
  const internal = globalThis.__TAURI_INTERNALS__ && globalThis.__TAURI_INTERNALS__.invoke;
  return typeof direct === "function" || typeof internal === "function";
}

export function invokeTauriConfirmDialog(message) {
  const invoke =
    (globalThis.__TAURI__ && globalThis.__TAURI__.core && globalThis.__TAURI__.core.invoke) ||
    (globalThis.__TAURI_INTERNALS__ && globalThis.__TAURI_INTERNALS__.invoke);
  if (typeof invoke !== "function") {
    throw new Error("Tauri invoke unavailable");
  }
  return invoke("confirm_delete_session_via_dialog", { message });
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = hasTauriInvokeForConfirm)]
    fn has_tauri_invoke_for_confirm() -> bool;
    #[wasm_bindgen(js_name = invokeTauriConfirmDialog)]
    fn invoke_tauri_confirm_dialog(message: &str) -> js_sys::Promise;
}

fn running_in_tauri_webview() -> bool {
    let Some(w) = web_sys::window() else {
        return false;
    };
    let has_tauri = js_sys::Reflect::get(&w, &wasm_bindgen::JsValue::from_str("__TAURI__"))
        .ok()
        .is_some_and(|v| !v.is_null() && !v.is_undefined());
    let has_internals =
        js_sys::Reflect::get(&w, &wasm_bindgen::JsValue::from_str("__TAURI_INTERNALS__"))
            .ok()
            .is_some_and(|v| !v.is_null() && !v.is_undefined());
    has_tauri || has_internals
}

/// 用户确认返回 `true`；取消或对话框不可用返回 `false`。
pub async fn confirm_user_message(message: &str) -> bool {
    if running_in_tauri_webview() && has_tauri_invoke_for_confirm() {
        return match JsFuture::from(invoke_tauri_confirm_dialog(message)).await {
            Ok(v) => v.as_bool().unwrap_or(false),
            Err(_) => false,
        };
    }
    web_sys::window()
        .and_then(|w| w.confirm_with_message(message).ok())
        .unwrap_or(false)
}
