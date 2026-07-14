//! CrabMate Desktop（Tauri WebView）壳层能力：检测与窗口装饰等。

use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen_futures::{JsFuture, spawn_local};

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(inline_js = r#"
export function hasTauriInvoke() {
  const direct = globalThis.__TAURI__ && globalThis.__TAURI__.core && globalThis.__TAURI__.core.invoke;
  const internal = globalThis.__TAURI_INTERNALS__ && globalThis.__TAURI_INTERNALS__.invoke;
  return typeof direct === "function" || typeof internal === "function";
}

export function isLinuxPlatform() {
  const platform = (navigator.platform || "").toLowerCase();
  const ua = (navigator.userAgent || "").toLowerCase();
  return platform.includes("linux") || ua.includes("linux") || ua.includes("x11");
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

export function invokeTauriOpenWebviewUrl(url, title) {
  return tauriInvoke("open_webview_url", { url, title: title ?? null });
}

export function invokeTauriSyncGithubEmbed(url, x, y, width, height) {
  return tauriInvoke("sync_github_embed_webview", { url, x, y, width, height });
}

export function invokeTauriUnmountGithubEmbed() {
  return tauriInvoke("unmount_github_embed_webview", {});
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
#[cfg(target_arch = "wasm32")]
extern "C" {
    #[wasm_bindgen(js_name = hasTauriInvoke)]
    fn has_tauri_invoke() -> bool;
    #[wasm_bindgen(js_name = isLinuxPlatform)]
    fn is_linux_platform() -> bool;
    #[wasm_bindgen(js_name = invokeTauriSetMainWindowDecorations)]
    fn invoke_tauri_set_main_window_decorations(decorations: bool) -> js_sys::Promise;
    #[wasm_bindgen(js_name = invokeTauriMainWindowMinimize)]
    fn invoke_tauri_main_window_minimize() -> js_sys::Promise;
    #[wasm_bindgen(js_name = invokeTauriMainWindowToggleMaximize)]
    fn invoke_tauri_main_window_toggle_maximize() -> js_sys::Promise;
    #[wasm_bindgen(js_name = invokeTauriMainWindowClose)]
    fn invoke_tauri_main_window_close() -> js_sys::Promise;
    #[wasm_bindgen(js_name = invokeTauriOpenExternalUrl)]
    fn invoke_tauri_open_external_url(url: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name = invokeTauriOpenWebviewUrl)]
    fn invoke_tauri_open_webview_url(url: &str, title: Option<String>) -> js_sys::Promise;
    #[wasm_bindgen(js_name = invokeTauriSyncGithubEmbed)]
    fn invoke_tauri_sync_github_embed(
        url: &str,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> js_sys::Promise;
    #[wasm_bindgen(js_name = invokeTauriUnmountGithubEmbed)]
    fn invoke_tauri_unmount_github_embed() -> js_sys::Promise;
    #[wasm_bindgen(js_name = installChatExternalLinkHandler)]
    fn install_chat_external_link_handler();
}

// Native stubs for non-wasm targets (tests / SSR)
#[cfg(not(target_arch = "wasm32"))]
fn has_tauri_invoke() -> bool {
    false
}
#[cfg(not(target_arch = "wasm32"))]
fn is_linux_platform() -> bool {
    false
}
#[cfg(not(target_arch = "wasm32"))]
fn install_chat_external_link_handler() {}
#[cfg(not(target_arch = "wasm32"))]
fn invoke_tauri_set_main_window_decorations(_: bool) -> js_sys::Promise {
    js_sys::Promise::resolve(&wasm_bindgen::JsValue::UNDEFINED)
}
#[cfg(not(target_arch = "wasm32"))]
fn invoke_tauri_main_window_minimize() -> js_sys::Promise {
    js_sys::Promise::resolve(&wasm_bindgen::JsValue::UNDEFINED)
}
#[cfg(not(target_arch = "wasm32"))]
fn invoke_tauri_main_window_toggle_maximize() -> js_sys::Promise {
    js_sys::Promise::resolve(&wasm_bindgen::JsValue::UNDEFINED)
}
#[cfg(not(target_arch = "wasm32"))]
fn invoke_tauri_main_window_close() -> js_sys::Promise {
    js_sys::Promise::resolve(&wasm_bindgen::JsValue::UNDEFINED)
}
#[cfg(not(target_arch = "wasm32"))]
fn invoke_tauri_open_external_url(_: &str) -> js_sys::Promise {
    js_sys::Promise::resolve(&wasm_bindgen::JsValue::UNDEFINED)
}
#[cfg(not(target_arch = "wasm32"))]
fn invoke_tauri_open_webview_url(_: &str, _: Option<String>) -> js_sys::Promise {
    js_sys::Promise::resolve(&wasm_bindgen::JsValue::UNDEFINED)
}
#[cfg(not(target_arch = "wasm32"))]
fn invoke_tauri_sync_github_embed(_: &str, _: f64, _: f64, _: f64, _: f64) -> js_sys::Promise {
    js_sys::Promise::resolve(&wasm_bindgen::JsValue::UNDEFINED)
}
#[cfg(not(target_arch = "wasm32"))]
fn invoke_tauri_unmount_github_embed() -> js_sys::Promise {
    js_sys::Promise::resolve(&wasm_bindgen::JsValue::UNDEFINED)
}

/// 是否在 Tauri 桌面 WebView 内运行。
#[must_use]
pub fn tauri_shell_available() -> bool {
    has_tauri_invoke()
}

/// 是否在 Linux Tauri 桌面壳内运行。
#[must_use]
pub fn tauri_linux_shell_available() -> bool {
    tauri_shell_available() && is_linux_platform()
}

async fn tauri_invoke_void(promise: js_sys::Promise) -> Result<(), String> {
    JsFuture::from(promise)
        .await
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

async fn tauri_invoke_bool(promise: js_sys::Promise) -> Result<bool, String> {
    JsFuture::from(promise)
        .await
        .map_err(|e| format!("{e:?}"))?
        .as_bool()
        .ok_or_else(|| "Tauri invoke did not return a boolean".to_string())
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

/// 在系统默认浏览器中打开 URL（Tauri）。
pub fn tauri_open_external_url(url: &str) {
    if !tauri_shell_available() {
        let window = web_sys::window().expect("window");
        let _ = window.open_with_url_and_target(url, "_blank");
        return;
    }
    let url = url.to_string();
    spawn_local(async move {
        let _ = tauri_invoke_void(invoke_tauri_open_external_url(&url)).await;
    });
}

/// 在独立 WebView 窗口打开 GitHub 页面（Tauri）；浏览器模式下降级为新标签页。
#[allow(dead_code)]
pub fn tauri_open_github_webview(url: &str, title: Option<&str>) {
    if !tauri_shell_available() {
        tauri_open_external_url(url);
        return;
    }
    let url = url.to_string();
    let title = title.map(str::to_string);
    spawn_local(async move {
        let _ = tauri_invoke_void(invoke_tauri_open_webview_url(&url, title)).await;
    });
}

/// 挂载 GitHub WebView；返回 `true` 表示嵌入主窗口，`false` 表示使用独立窗口。
pub async fn tauri_mount_github_embed(
    url: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<bool, String> {
    tauri_invoke_bool(invoke_tauri_sync_github_embed(url, x, y, width, height)).await
}

/// 卸载主窗口内的 GitHub 嵌入子 WebView（仅 Tauri）。
pub fn tauri_unmount_github_embed() {
    if !tauri_shell_available() {
        return;
    }
    spawn_local(async move {
        let _ = tauri_invoke_void(invoke_tauri_unmount_github_embed()).await;
    });
}
