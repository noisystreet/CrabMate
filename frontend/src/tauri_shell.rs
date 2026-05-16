//! CrabMate Desktop（Tauri WebView）壳层能力：检测与窗口装饰等。

use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen_futures::{JsFuture, spawn_local};

#[wasm_bindgen(inline_js = r#"
export function hasTauriInvoke() {
  const direct = globalThis.__TAURI__ && globalThis.__TAURI__.core && globalThis.__TAURI__.core.invoke;
  const internal = globalThis.__TAURI_INTERNALS__ && globalThis.__TAURI_INTERNALS__.invoke;
  return typeof direct === "function" || typeof internal === "function";
}

function tauriInvoke(cmd, args) {
  const invoke =
    (globalThis.__TAURI__ && globalThis.__TAURI__.core && globalThis.__TAURI__.core.invoke) ||
    (globalThis.__TAURI_INTERNALS__ && globalThis.__TAURI_INTERNALS__.invoke);
  if (typeof invoke !== "function") {
    throw new Error("Tauri invoke unavailable");
  }
  return invoke(cmd, args);
}

export function invokeTauriSetMainWindowDecorations(decorations) {
  return tauriInvoke("set_main_window_decorations", { decorations });
}

export function invokeTauriMainWindowMinimize() {
  return tauriInvoke("main_window_minimize", {});
}

export function invokeTauriMainWindowToggleMaximize() {
  return tauriInvoke("main_window_toggle_maximize", {});
}

export function invokeTauriMainWindowClose() {
  return tauriInvoke("main_window_close", {});
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = hasTauriInvoke)]
    fn has_tauri_invoke() -> bool;
    #[wasm_bindgen(js_name = invokeTauriSetMainWindowDecorations)]
    fn invoke_tauri_set_main_window_decorations(decorations: bool) -> js_sys::Promise;
    #[wasm_bindgen(js_name = invokeTauriMainWindowMinimize)]
    fn invoke_tauri_main_window_minimize() -> js_sys::Promise;
    #[wasm_bindgen(js_name = invokeTauriMainWindowToggleMaximize)]
    fn invoke_tauri_main_window_toggle_maximize() -> js_sys::Promise;
    #[wasm_bindgen(js_name = invokeTauriMainWindowClose)]
    fn invoke_tauri_main_window_close() -> js_sys::Promise;
}

/// 是否在 Tauri 桌面 WebView 内运行。
#[must_use]
pub fn tauri_shell_available() -> bool {
    has_tauri_invoke()
}

async fn tauri_invoke_void(promise: js_sys::Promise) -> Result<(), String> {
    JsFuture::from(promise)
        .await
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

/// IDE 布局下隐藏系统标题栏（保留自定义窗口按钮）；对话布局恢复装饰。
pub async fn tauri_set_main_window_decorations(decorations: bool) -> Result<(), String> {
    tauri_invoke_void(invoke_tauri_set_main_window_decorations(decorations)).await
}

/// 根据 IDE 布局切换 Tauri 窗口装饰（异步，忽略错误以免阻塞 UI）。
pub fn tauri_apply_ide_layout_window_chrome(editor_layout_mode: bool) {
    if !tauri_shell_available() {
        return;
    }
    let decorations = !editor_layout_mode;
    spawn_local(async move {
        let _ = tauri_set_main_window_decorations(decorations).await;
    });
}

fn tauri_spawn_window_action(f: fn() -> js_sys::Promise) {
    if !tauri_shell_available() {
        return;
    }
    spawn_local(async move {
        let _ = tauri_invoke_void(f()).await;
    });
}

/// 最小化主窗口（Tauri）。
pub fn tauri_main_window_minimize() {
    tauri_spawn_window_action(invoke_tauri_main_window_minimize);
}

/// 切换主窗口最大化（Tauri）。
pub fn tauri_main_window_toggle_maximize() {
    tauri_spawn_window_action(invoke_tauri_main_window_toggle_maximize);
}

/// 关闭主窗口（Tauri）。
pub fn tauri_main_window_close() {
    tauri_spawn_window_action(invoke_tauri_main_window_close);
}
