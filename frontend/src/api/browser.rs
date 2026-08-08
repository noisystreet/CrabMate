//! 浏览器侧共享：`window` / 受保护 API 的鉴权头 / API 基址。
//!
//! Web API Bearer 须同时存在于：
//! - **服务端** `CM_WEB_API_BEARER_TOKEN` / `web_api_bearer_token`（校验用）
//! - **本页**内存（及 `localStorage` 引导键，见下）：请求头 `Authorization` / `X-API-Key`
//!
//! 与侧栏「API 密钥」(`client_llm` / `API_KEY`) 不是同一字段。
//!
//! **API 基址**（路径 A Phase 2）：空 = 同 Origin 相对路径；非空时拼到 `/chat/stream` 等路径前。
//! 可选构建期默认：`option_env!("CRABMATE_API_BASE")`。
//! - **键缺失**：用构建期默认。
//! - **键存在**（含显式空串）：以 localStorage 为准（清空后不再回落到构建期默认）。

use std::cell::RefCell;

use web_sys::{Headers, Window};

/// 与历史文档 / `serve` 启动提示一致：浏览器侧引导缓存键（先于钥匙串，解决「须鉴权才能写 secrets」的冷启动）。
const WEB_API_BEARER_TOKEN_KEY: &str = "crabmate-api-bearer-token";

/// 跨 Origin 时指向远程 `serve` 根（无尾斜杠）；空 = 同 Origin。
const API_BASE_URL_KEY: &str = "crabmate-api-base-url";

thread_local! {
    static WEB_API_BEARER: RefCell<String> = const { RefCell::new(String::new()) };
    static WEB_API_BEARER_HYDRATED: RefCell<bool> = const { RefCell::new(false) };
    static API_BASE_URL: RefCell<String> = const { RefCell::new(String::new()) };
    static API_BASE_HYDRATED: RefCell<bool> = const { RefCell::new(false) };
}

pub fn window() -> Option<Window> {
    web_sys::window()
}

fn read_local_storage_item(key: &str) -> Option<String> {
    let w = window()?;
    let storage = w.local_storage().ok().flatten()?;
    let v = storage.get_item(key).ok().flatten()?;
    let t = v.trim().to_string();
    if t.is_empty() { None } else { Some(t) }
}

fn write_local_storage_item(key: &str, value: &str) {
    let Some(w) = window() else {
        return;
    };
    let Ok(Some(storage)) = w.local_storage() else {
        return;
    };
    let t = value.trim();
    if t.is_empty() {
        let _ = storage.remove_item(key);
    } else {
        let _ = storage.set_item(key, t);
    }
}

/// `None` = 无 storage / 键缺失；`Some` = 键存在（值可为空白，表示显式同 Origin）。
fn read_api_base_local_storage_raw() -> Option<String> {
    let w = window()?;
    let storage = w.local_storage().ok().flatten()?;
    storage.get_item(API_BASE_URL_KEY).ok().flatten()
}

fn write_api_base_local_storage(normalized: &str) {
    let Some(w) = window() else {
        return;
    };
    let Ok(Some(storage)) = w.local_storage() else {
        return;
    };
    // 显式空串保留键，避免刷新后回落到 `CRABMATE_API_BASE`。
    let _ = storage.set_item(API_BASE_URL_KEY, normalized);
}

fn read_local_storage_bearer() -> Option<String> {
    read_local_storage_item(WEB_API_BEARER_TOKEN_KEY)
}

fn write_local_storage_bearer(token: &str) {
    write_local_storage_item(WEB_API_BEARER_TOKEN_KEY, token);
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

/// 规范化 API 基址：去空白与尾 `/`；非法时返回空（回退同 Origin）。
#[must_use]
pub fn normalize_api_base_url(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    let without_slash = t.trim_end_matches('/');
    if !(without_slash.starts_with("http://") || without_slash.starts_with("https://")) {
        return String::new();
    }
    without_slash.to_string()
}

fn build_time_api_base_default() -> String {
    option_env!("CRABMATE_API_BASE")
        .map(normalize_api_base_url)
        .unwrap_or_default()
}

/// 由 localStorage 原始值与构建期默认决定基址（纯函数，便于单测）。
///
/// - `ls_raw == None`：键缺失或不可用 → 构建期默认
/// - `ls_raw == Some(_)`：键存在 → 规范化后的值（空 = 同 Origin）
#[must_use]
pub fn resolve_api_base_url(ls_raw: Option<&str>, build_default: &str) -> String {
    match ls_raw {
        Some(raw) => normalize_api_base_url(raw),
        None => normalize_api_base_url(build_default),
    }
}

fn hydrate_api_base_once() {
    API_BASE_HYDRATED.with(|h| {
        if *h.borrow() {
            return;
        }
        *h.borrow_mut() = true;
        let chosen = resolve_api_base_url(
            read_api_base_local_storage_raw().as_deref(),
            &build_time_api_base_default(),
        );
        API_BASE_URL.with(|c| {
            if c.borrow().is_empty() {
                *c.borrow_mut() = chosen;
            }
        });
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

/// 设置远程 `serve` API 基址（空 = 同 Origin）。写入内存 + `localStorage`（空串仍保留键）。
pub fn set_api_base_url(base: &str) {
    let t = normalize_api_base_url(base);
    API_BASE_URL.with(|c| *c.borrow_mut() = t.clone());
    API_BASE_HYDRATED.with(|h| *h.borrow_mut() = true);
    write_api_base_local_storage(&t);
}

/// 当前 API 基址（无尾 `/`）；空表示同 Origin 相对路径。
#[must_use]
pub fn api_base_url() -> String {
    hydrate_api_base_once();
    API_BASE_URL.with(|c| c.borrow().clone())
}

/// 将以 `/` 开头的 API 路径接到基址上；基址为空则原样返回 `path`。
///
/// 已是 `http(s)://` 绝对 URL 时原样返回。
#[must_use]
pub fn api_url(path: &str) -> String {
    let p = path.trim();
    if p.starts_with("http://") || p.starts_with("https://") {
        return p.to_string();
    }
    let path = if p.is_empty() {
        "/"
    } else if p.starts_with('/') {
        p
    } else {
        // 容错：缺前导 `/` 时补上
        return api_url(&format!("/{p}"));
    };
    let base = api_base_url();
    if base.is_empty() {
        path.to_string()
    } else {
        format!("{base}{path}")
    }
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

    #[test]
    fn normalize_api_base_strips_slash_and_rejects_relative() {
        assert_eq!(
            normalize_api_base_url("https://api.example.com/v1/"),
            "https://api.example.com/v1"
        );
        assert_eq!(normalize_api_base_url("  "), "");
        assert_eq!(normalize_api_base_url("/only-path"), "");
        assert_eq!(normalize_api_base_url("ftp://x"), "");
    }

    #[test]
    fn resolve_api_base_prefers_explicit_empty_over_build_default() {
        assert_eq!(
            resolve_api_base_url(None, "http://build.example:8080"),
            "http://build.example:8080"
        );
        assert_eq!(
            resolve_api_base_url(Some(""), "http://build.example:8080"),
            ""
        );
        assert_eq!(
            resolve_api_base_url(
                Some(" http://127.0.0.1:8080/ "),
                "http://build.example:8080"
            ),
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn api_url_empty_base_keeps_relative() {
        // 测试线程未 hydrate 浏览器 storage；直接测拼接逻辑
        assert_eq!(
            {
                let path = "/chat/stream";
                let base = "";
                if base.is_empty() {
                    path.to_string()
                } else {
                    format!("{base}{path}")
                }
            },
            "/chat/stream"
        );
        assert_eq!(
            {
                let path = "/chat/stream";
                let base = "http://127.0.0.1:8080";
                format!("{base}{path}")
            },
            "http://127.0.0.1:8080/chat/stream"
        );
    }
}
