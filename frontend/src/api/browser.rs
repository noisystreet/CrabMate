//! 浏览器侧共享：`window` / 受保护 API 的鉴权头（Bearer 仅存进程内存 + 服务端 `secrets/`）。

use std::cell::RefCell;

use web_sys::{Headers, Window};

thread_local! {
    static WEB_API_BEARER: RefCell<String> = const { RefCell::new(String::new()) };
}

pub fn window() -> Option<Window> {
    web_sys::window()
}

/// 设置本进程内访问 CrabMate HTTP API 的 Bearer（并应 `PUT /user-data/secrets/web-api-bearer`）。
#[allow(dead_code)]
pub fn set_web_api_bearer_token(token: &str) {
    WEB_API_BEARER.with(|c| *c.borrow_mut() = token.trim().to_string());
}

#[must_use]
pub fn web_api_bearer_token() -> String {
    WEB_API_BEARER.with(|c| c.borrow().clone())
}

pub fn auth_headers() -> Headers {
    let h = Headers::new().expect("Headers::new");
    let t = web_api_bearer_token();
    if !t.is_empty() {
        let _ = h.set("Authorization", &format!("Bearer {t}"));
        let _ = h.set("X-API-Key", &t);
    }
    h
}
