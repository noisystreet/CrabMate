//! Android 远程薄客户端桥：`MainActivity` 注入的 `window.CrabMateMobile`。
//! 远程 `serve` 源上无 Tauri IPC，断开须走此桥（或系统返回键）。

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(inline_js = r#"
/** 是否在 Android 远程薄客户端 WebView（含已导航到远程 serve 的页面）。 */
export function isCrabMateMobileRemoteClient() {
  try {
    const b = globalThis.CrabMateMobile;
    if (!b) return false;
    if (typeof b.isRemoteClient === "function") {
      return !!b.isRemoteClient();
    }
    return typeof b.disconnect === "function";
  } catch (_) {
    return false;
  }
}

export function hasCrabMateMobileDisconnect() {
  try {
    return !!(
      globalThis.CrabMateMobile &&
      typeof globalThis.CrabMateMobile.disconnect === "function"
    );
  } catch (_) {
    return false;
  }
}

export function invokeCrabMateMobileDisconnect() {
  if (
    !globalThis.CrabMateMobile ||
    typeof globalThis.CrabMateMobile.disconnect !== "function"
  ) {
    throw new Error("CrabMateMobile.disconnect unavailable");
  }
  globalThis.CrabMateMobile.disconnect();
}

export function hasCrabMateMobileOpenExternalUrl() {
  try {
    return !!(
      globalThis.CrabMateMobile &&
      typeof globalThis.CrabMateMobile.openExternalUrl === "function"
    );
  } catch (_) {
    return false;
  }
}

export function invokeCrabMateMobileOpenExternalUrl(url) {
  if (
    !globalThis.CrabMateMobile ||
    typeof globalThis.CrabMateMobile.openExternalUrl !== "function"
  ) {
    throw new Error("CrabMateMobile.openExternalUrl unavailable");
  }
  globalThis.CrabMateMobile.openExternalUrl(String(url || ""));
}

/** 从原生读取顶栏/底栏安全区并写入 CSS 变量。可多次调用。 */
export function applyCrabMateMobileSafeTop() {
  try {
    const b = globalThis.CrabMateMobile;
    if (!b || typeof b.getStatusBarInsetPx !== "function") {
      return false;
    }
    let top = Number(b.getStatusBarInsetPx());
    if (!Number.isFinite(top) || top < 0) {
      top = 24;
    }
    top = Math.max(top, 24);
    let bottom = 24;
    if (typeof b.getNavBarInsetPx === "function") {
      bottom = Number(b.getNavBarInsetPx());
      if (!Number.isFinite(bottom) || bottom < 0) {
        bottom = 24;
      }
      bottom = Math.max(bottom, 24);
    }
    const root = document.documentElement;
    root.style.setProperty("--cm-safe-top", top + "px");
    root.style.setProperty("--cm-safe-bottom", bottom + "px");
    root.setAttribute("data-cm-mobile-shell", "");
    return true;
  } catch (_) {
    return false;
  }
}

/** 启动时重试：桥或 insets 可能晚于 WASM bootstrap。 */
export function scheduleCrabMateMobileSafeTop() {
  const run = () => applyCrabMateMobileSafeTop();
  if (run()) return true;
  [50, 200, 600, 1500].forEach((ms) => setTimeout(run, ms));
  return false;
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = isCrabMateMobileRemoteClient)]
    fn is_crabmate_mobile_remote_client() -> bool;
    #[wasm_bindgen(js_name = hasCrabMateMobileDisconnect)]
    fn has_crabmate_mobile_disconnect() -> bool;
    #[wasm_bindgen(js_name = invokeCrabMateMobileDisconnect)]
    fn invoke_crabmate_mobile_disconnect();
    #[wasm_bindgen(js_name = hasCrabMateMobileOpenExternalUrl)]
    fn has_crabmate_mobile_open_external_url() -> bool;
    #[wasm_bindgen(js_name = invokeCrabMateMobileOpenExternalUrl)]
    fn invoke_crabmate_mobile_open_external_url(url: &str);
    #[wasm_bindgen(js_name = scheduleCrabMateMobileSafeTop)]
    fn schedule_crabmate_mobile_safe_top() -> bool;
}

#[cfg(not(target_arch = "wasm32"))]
fn is_crabmate_mobile_remote_client() -> bool {
    false
}

#[cfg(not(target_arch = "wasm32"))]
fn has_crabmate_mobile_disconnect() -> bool {
    false
}

#[cfg(not(target_arch = "wasm32"))]
fn invoke_crabmate_mobile_disconnect() {}

#[cfg(not(target_arch = "wasm32"))]
fn has_crabmate_mobile_open_external_url() -> bool {
    false
}

#[cfg(not(target_arch = "wasm32"))]
fn invoke_crabmate_mobile_open_external_url(_: &str) {}

#[cfg(not(target_arch = "wasm32"))]
fn schedule_crabmate_mobile_safe_top() -> bool {
    false
}

/// 是否在 Android 远程薄客户端 WebView 内（`CrabMateMobile`）。
#[must_use]
pub fn mobile_remote_client() -> bool {
    is_crabmate_mobile_remote_client()
}

/// 是否可断开回连接页（同 [`mobile_remote_client`] 的常见能力探测）。
#[must_use]
pub fn mobile_remote_disconnect_available() -> bool {
    has_crabmate_mobile_disconnect()
}

/// 请求壳导航回本地连接页。
pub fn mobile_remote_disconnect() {
    if !has_crabmate_mobile_disconnect() {
        return;
    }
    invoke_crabmate_mobile_disconnect();
}

/// 经原生桥在系统浏览器打开 URL；不可用时返回 `false`。
#[must_use]
pub fn mobile_remote_open_external_url(url: &str) -> bool {
    let u = url.trim();
    if u.is_empty() || !has_crabmate_mobile_open_external_url() {
        return false;
    }
    invoke_crabmate_mobile_open_external_url(u);
    true
}

/// 应用 Android 顶栏安全区 CSS 变量（尽早调用，并安排短延迟重试）。
pub fn apply_mobile_remote_safe_top() {
    let _ = schedule_crabmate_mobile_safe_top();
}
