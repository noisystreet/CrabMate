//! Android 远程薄客户端桥：`MainActivity` 注入的 `window.CrabMateMobile`。
//! 远程 `serve` 源上无 Tauri IPC，断开须走此桥（或系统返回键）。

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(inline_js = r#"
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
"#)]
extern "C" {
    #[wasm_bindgen(js_name = hasCrabMateMobileDisconnect)]
    fn has_crabmate_mobile_disconnect() -> bool;
    #[wasm_bindgen(js_name = invokeCrabMateMobileDisconnect)]
    fn invoke_crabmate_mobile_disconnect();
}

#[cfg(not(target_arch = "wasm32"))]
fn has_crabmate_mobile_disconnect() -> bool {
    false
}

#[cfg(not(target_arch = "wasm32"))]
fn invoke_crabmate_mobile_disconnect() {}

/// 是否在 Android 壳内浏览远程 UI（可断开回连接页）。
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
