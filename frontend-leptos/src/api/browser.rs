//! 浏览器侧共享：`window` / `localStorage` / 受保护 API 的鉴权头。

use web_sys::{Headers, Window};

const WEB_API_BEARER_TOKEN_KEY: &str = "crabmate-api-bearer-token";

pub fn window() -> Option<Window> {
    web_sys::window()
}

pub fn local_storage() -> Option<web_sys::Storage> {
    window().and_then(|w| w.local_storage().ok().flatten())
}

pub fn auth_headers() -> Headers {
    let h = Headers::new().expect("Headers::new");
    if let Some(st) = local_storage() {
        if let Ok(Some(t)) = st.get_item(WEB_API_BEARER_TOKEN_KEY) {
            let t = t.trim();
            if !t.is_empty() {
                let _ = h.set("Authorization", &format!("Bearer {t}"));
                // 与后端 `require_web_api_bearer_auth` 一致：亦接受 X-API-Key（网关/脚本常用）
                let _ = h.set("X-API-Key", t);
            }
        }
    }
    h
}
