//! 浏览器 / Tauri / 移动 WebView 通用确认框。
//!
//! 桌面与 Android WebView 上 `window.confirm` 常无效或恒为 false；优先 Tauri 原生对话框，
//! 否则使用壳层内嵌确认框（由 [`register_shell_confirm`] 在启动时挂接）。

use std::cell::Cell;

use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen_futures::JsFuture;

use crate::ide_confirm::{IdeConfirmSignals, ide_confirm_user};

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

thread_local! {
    static SHELL_CONFIRM: Cell<Option<IdeConfirmSignals>> = const { Cell::new(None) };
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

/// 在 App 启动时注册壳层确认框信号（可重复调用以覆盖）。
pub fn register_shell_confirm(signals: IdeConfirmSignals) {
    SHELL_CONFIRM.set(Some(signals));
}

async fn confirm_via_shell_or_window(message: &str, ok_label: &str, cancel_label: &str) -> bool {
    if let Some(signals) = SHELL_CONFIRM.get() {
        return ide_confirm_user(
            signals,
            message.to_string(),
            ok_label.to_string(),
            cancel_label.to_string(),
        )
        .await;
    }
    web_sys::window()
        .and_then(|w| w.confirm_with_message(message).ok())
        .unwrap_or(false)
}

/// 用户确认返回 `true`；取消或对话框不可用返回 `false`。
///
/// `ok_label` / `cancel_label` 仅用于壳层内嵌确认框；Tauri 原生对话框仍用系统默认按钮。
pub async fn confirm_user_message(message: &str, ok_label: &str, cancel_label: &str) -> bool {
    if running_in_tauri_webview() && has_tauri_invoke_for_confirm() {
        match JsFuture::from(invoke_tauri_confirm_dialog(message)).await {
            Ok(v) => return v.as_bool().unwrap_or(false),
            // 桥接/命令失败时回退壳层确认，避免删除静默无响应
            Err(_) => {
                return confirm_via_shell_or_window(message, ok_label, cancel_label).await;
            }
        }
    }
    confirm_via_shell_or_window(message, ok_label, cancel_label).await
}
