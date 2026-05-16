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

export function invokeTauriOpenExternalUrl(url) {
  return tauriInvoke("open_external_url", { url });
}

export function installChatExternalLinkHandler() {
  if (globalThis.__crabmateChatExternalLinkHandlerInstalled) {
    return;
  }
  globalThis.__crabmateChatExternalLinkHandlerInstalled = true;
  document.addEventListener(
    "click",
    (ev) => {
      const target = ev.target;
      if (!target || typeof target.closest !== "function") {
        return;
      }
      const anchor = target.closest("a[href]");
      if (!anchor) {
        return;
      }
      const raw = anchor.getAttribute("href");
      if (!raw || raw.startsWith('#')) {
        return;
      }
      let parsed;
      try {
        parsed = new URL(raw, window.location.href);
      } catch {
        return;
      }
      const scheme = parsed.protocol.replace(":", "");
      if (scheme !== "http" && scheme !== "https" && scheme !== "mailto") {
        return;
      }
      if (parsed.origin === window.location.origin) {
        return;
      }
      ev.preventDefault();
      ev.stopPropagation();
      void invokeTauriOpenExternalUrl(parsed.href);
    },
    true
  );
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
    #[wasm_bindgen(js_name = installChatExternalLinkHandler)]
    fn install_chat_external_link_handler();
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

/// 隐藏系统标题栏（保留应用内最小化/最大化/关闭按钮）。
pub async fn tauri_set_main_window_decorations(decorations: bool) -> Result<(), String> {
    tauri_invoke_void(invoke_tauri_set_main_window_decorations(decorations)).await
}

/// Tauri 启动后始终使用无边框主窗口（会话与 IDE 模式一致）。
pub fn tauri_apply_frameless_window_chrome() {
    if !tauri_shell_available() {
        return;
    }
    install_chat_external_link_handler();
    spawn_local(async move {
        let _ = tauri_set_main_window_decorations(false).await;
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
