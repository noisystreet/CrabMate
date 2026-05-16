use reqwest::Url;

use super::args::{FetchMethod, RequestMethod};

/// 永久允许列表与审批判定的键（小写、无 query/fragment）；**仅** `http_fetch`（GET/HEAD）使用。
pub fn storage_key(url: &Url) -> String {
    let mut u = url.clone();
    u.set_query(None);
    u.set_fragment(None);
    format!("http_fetch:{}", u.as_str().to_lowercase())
}

/// `http_request`（POST/PUT/PATCH/DELETE）审批白名单键：含 **HTTP 方法**，避免与同源 URL 的 `http_fetch` 键混用。
pub fn request_storage_key(method: RequestMethod, url: &Url) -> String {
    let mut u = url.clone();
    u.set_query(None);
    u.set_fragment(None);
    format!(
        "http_request:{}:{}",
        method.as_str(),
        u.as_str().to_lowercase()
    )
}

/// 审批界面与日志用：隐藏 query 内容
pub fn display_redacted(url: &Url) -> String {
    let mut u = url.clone();
    if u.query().is_some() {
        u.set_query(Some("…"));
    }
    u.set_fragment(None);
    u.to_string()
}

/// TUI 审批 `args` 文案：HEAD 时前缀方法名。
pub fn approval_args_display(method: FetchMethod, url: &Url) -> String {
    let r = display_redacted(url);
    match method {
        FetchMethod::Get => r,
        FetchMethod::Head => format!("HEAD {}", r),
    }
}

/// `http_request` 审批展示：方法 + 脱敏 URL；不展示 body 内容。
pub fn approval_args_display_request(
    method: RequestMethod,
    url: &Url,
    has_json_body: bool,
) -> String {
    let r = display_redacted(url);
    let mut s = format!("{} {}", method.as_str(), r);
    if has_json_body {
        s.push_str("（含 json_body）");
    }
    s
}

fn url_path_matches_prefix(url_path: &str, prefix_path: &str) -> bool {
    if prefix_path.ends_with('/') {
        return url_path.starts_with(prefix_path);
    }
    url_path == prefix_path || url_path.starts_with(&format!("{}/", prefix_path))
}

fn same_origin(url: &Url, prefix: &Url) -> bool {
    let port_url = url.port_or_known_default();
    let port_prefix = prefix.port_or_known_default();
    url.scheme() == prefix.scheme()
        && url.host_str() == prefix.host_str()
        && port_url == port_prefix
}

pub fn url_matches_allowed_prefixes(url: &Url, prefixes: &[String]) -> bool {
    prefixes.iter().any(|raw| {
        let p = raw.trim();
        if p.is_empty() {
            return false;
        }
        let Ok(prefix) = Url::parse(p) else {
            return false;
        };
        if prefix.query().is_some() || prefix.fragment().is_some() {
            return false;
        }
        same_origin(url, &prefix) && url_path_matches_prefix(url.path(), prefix.path())
    })
}
