//! 浏览器侧共享：`window` / 受保护 API 的鉴权头。
//!
//! Web API Bearer 须同时存在于：
//! - **服务端** `CM_WEB_API_BEARER_TOKEN` / `web_api_bearer_token`（校验用）
//! - **本页**内存（及 `localStorage` 引导键，见下）：请求头 `Authorization` / `X-API-Key`
//!
//! 与侧栏「API 密钥」(`client_llm` / `API_KEY`) 不是同一字段。

use std::cell::RefCell;

use web_sys::{Headers, Window};

/// 与历史文档 / `serve` 启动提示一致：浏览器侧引导缓存键（先于钥匙串，解决「须鉴权才能写 secrets」的冷启动）。
const WEB_API_BEARER_TOKEN_KEY: &str = "crabmate-api-bearer-token";

thread_local! {
    static WEB_API_BEARER: RefCell<String> = const { RefCell::new(String::new()) };
    static WEB_API_BEARER_HYDRATED: RefCell<bool> = const { RefCell::new(false) };
}

pub fn window() -> Option<Window> {
    web_sys::window()
}

fn read_local_storage_bearer() -> Option<String> {
    let w = window()?;
    let storage = w.local_storage().ok().flatten()?;
    let v = storage.get_item(WEB_API_BEARER_TOKEN_KEY).ok().flatten()?;
    let t = v.trim().to_string();
    if t.is_empty() { None } else { Some(t) }
}

fn write_local_storage_bearer(token: &str) {
    let Some(w) = window() else {
        return;
    };
    let Ok(Some(storage)) = w.local_storage() else {
        return;
    };
    let t = token.trim();
    if t.is_empty() {
        let _ = storage.remove_item(WEB_API_BEARER_TOKEN_KEY);
    } else {
        let _ = storage.set_item(WEB_API_BEARER_TOKEN_KEY, t);
    }
}

fn hydrate_web_api_bearer_once() {
    WEB_API_BEARER_HYDRATED.with(|h| {
        if *h.borrow() {
            return;
        }
        *h.borrow_mut() = true;
        if let Some(t) = read_local_storage_bearer() {
            WEB_API_BEARER.with(|c| {
                if c.borrow().is_empty() {
                    *c.borrow_mut() = t;
                }
            });
        }
    });
}

/// 设置本页访问 CrabMate HTTP API 的 Bearer（写入内存 + `localStorage` 引导键）。
/// 值须与服务端 `CM_WEB_API_BEARER_TOKEN` **完全一致**。
pub fn set_web_api_bearer_token(token: &str) {
    let t = token.trim().to_string();
    WEB_API_BEARER.with(|c| *c.borrow_mut() = t.clone());
    WEB_API_BEARER_HYDRATED.with(|h| *h.borrow_mut() = true);
    write_local_storage_bearer(&t);
}

/// 当前内存中的 Web API Bearer（必要时从 `localStorage` 冷启动注入）。
#[must_use]
pub fn web_api_bearer_token() -> String {
    hydrate_web_api_bearer_once();
    WEB_API_BEARER.with(|c| c.borrow().clone())
}

/// 是否已配置非空 Web API Bearer（本页）。
#[must_use]
pub fn web_api_bearer_token_is_set() -> bool {
    !web_api_bearer_token().trim().is_empty()
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

/// 错误串是否像 **Web API 共享密钥** 校验失败（非模型 `API_KEY`）。
#[must_use]
pub fn is_web_api_credential_error(err: &str) -> bool {
    let low = err.to_ascii_lowercase();
    if low.contains("llm_api_key_required") {
        return false;
    }
    low.contains("web api")
        || low.contains("x-api-key")
        || low.contains("web bearer")
        || low.contains("web_api")
        || (low.contains("缺少或无效") && low.contains("凭证"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_server_web_api_credential_message() {
        assert!(is_web_api_credential_error(
            "请求失败 (401): 缺少或无效的 Web API 凭证（Authorization: Bearer 或 X-API-Key）"
        ));
        assert!(is_web_api_credential_error(
            "Request failed (401): missing or invalid Web API credentials"
        ));
    }

    #[test]
    fn detects_http_401_guide_from_api_err() {
        let zh = crate::i18n::api_err_http_status(
            crate::i18n::Locale::ZhHans,
            401,
            "缺少或无效的 Web API 凭证（Authorization: Bearer 或 X-API-Key）",
        );
        assert!(is_web_api_credential_error(&zh));
        let en = crate::i18n::api_err_http_status(crate::i18n::Locale::En, 401, "");
        assert!(is_web_api_credential_error(&en));
    }
}
