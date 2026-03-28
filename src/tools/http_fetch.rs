//! 受控 HTTP GET / HEAD（仅 https/http）；**CLI** 下 URL 未匹配 `http_fetch_allowed_prefixes` 时走 **`runtime::cli_approval`**（与 `run_command` 同套拒绝/一次/永久同意；**`--yes`** 亦跳过提示）。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::Url;
use reqwest::header::CONTENT_TYPE;
use reqwest::redirect::Policy;

use super::ToolContext;

/// 响应体硬上限（与配置 `http_fetch_max_response_bytes` 上界一致）
pub const ABS_MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
/// `http_request` JSON 请求体上限（字节，序列化后）
const MAX_REQUEST_JSON_BODY_BYTES: usize = 256 * 1024;

/// 与 `http_fetch` 工具对应的 HTTP 方法（默认 GET）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchMethod {
    Get,
    Head,
}

impl FetchMethod {
    fn as_str(self) -> &'static str {
        match self {
            FetchMethod::Get => "GET",
            FetchMethod::Head => "HEAD",
        }
    }
}

/// 与 `http_request` 工具对应的 HTTP 方法（仅变更类；GET/HEAD 请使用 `http_fetch`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestMethod {
    Post,
    Put,
    Patch,
    Delete,
}

impl RequestMethod {
    fn as_str(self) -> &'static str {
        match self {
            RequestMethod::Post => "POST",
            RequestMethod::Put => "PUT",
            RequestMethod::Patch => "PATCH",
            RequestMethod::Delete => "DELETE",
        }
    }

    fn into_reqwest(self) -> reqwest::Method {
        match self {
            RequestMethod::Post => reqwest::Method::POST,
            RequestMethod::Put => reqwest::Method::PUT,
            RequestMethod::Patch => reqwest::Method::PATCH,
            RequestMethod::Delete => reqwest::Method::DELETE,
        }
    }
}

/// 解析 `url` 与可选 `method`（`GET` / `HEAD`，默认 `GET`）。
pub fn parse_http_fetch_args(args_json: &str) -> Result<(Url, FetchMethod), String> {
    let v: serde_json::Value =
        serde_json::from_str(args_json).map_err(|e| format!("参数 JSON 无效: {}", e))?;
    let u = v
        .get("url")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "缺少 url".to_string())?;
    let url = Url::parse(u).map_err(|e| format!("URL 解析失败: {}", e))?;
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!("仅允许 http/https 方案，当前为 {}", scheme));
    }

    let method_upper = v
        .get("method")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_ascii_uppercase())
        .filter(|s| !s.is_empty());

    let method = match method_upper.as_deref() {
        None | Some("GET") => FetchMethod::Get,
        Some("HEAD") => FetchMethod::Head,
        Some(other) => {
            return Err(format!("method 仅支持 GET 或 HEAD（收到 {:?}）", other));
        }
    };

    Ok((url, method))
}

/// 解析 `http_request` 入参：`url` + `method`（POST/PUT/PATCH/DELETE）+ 可选 `json_body`。
pub fn parse_http_request_args(
    args_json: &str,
) -> Result<(Url, RequestMethod, Option<serde_json::Value>), String> {
    let v: serde_json::Value =
        serde_json::from_str(args_json).map_err(|e| format!("参数 JSON 无效: {}", e))?;
    let u = v
        .get("url")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "缺少 url".to_string())?;
    let url = Url::parse(u).map_err(|e| format!("URL 解析失败: {}", e))?;
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!("仅允许 http/https 方案，当前为 {}", scheme));
    }
    let method_raw = v
        .get("method")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_ascii_uppercase())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "缺少 method（POST/PUT/PATCH/DELETE）".to_string())?;
    let method = match method_raw.as_str() {
        "POST" => RequestMethod::Post,
        "PUT" => RequestMethod::Put,
        "PATCH" => RequestMethod::Patch,
        "DELETE" => RequestMethod::Delete,
        _ => {
            return Err(format!(
                "method 仅支持 POST/PUT/PATCH/DELETE（收到 {:?}）",
                method_raw
            ));
        }
    };
    let json_body = v.get("json_body").cloned();
    if let Some(body) = json_body.as_ref() {
        let body_len = serde_json::to_vec(body)
            .map(|b| b.len())
            .map_err(|e| format!("json_body 序列化失败: {}", e))?;
        if body_len > MAX_REQUEST_JSON_BODY_BYTES {
            return Err(format!(
                "json_body 过大：{} 字节（上限 {} 字节）",
                body_len, MAX_REQUEST_JSON_BODY_BYTES
            ));
        }
    }
    Ok((url, method, json_body))
}

/// 永久允许列表与审批判定的键（小写、无 query/fragment）；与 GET/HEAD 共用。
pub fn storage_key(url: &Url) -> String {
    let mut u = url.clone();
    u.set_query(None);
    u.set_fragment(None);
    format!("http_fetch:{}", u.as_str().to_lowercase())
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

fn build_client(
    timeout_secs: u64,
    redirect_hops: Arc<Mutex<Vec<String>>>,
) -> Result<reqwest::blocking::Client, String> {
    let timeout_secs = timeout_secs.max(1);
    let hops = redirect_hops;
    let policy = Policy::custom(move |attempt| {
        if attempt.previous().len() > 10 {
            return attempt.error("重定向次数过多（>10）");
        }
        if let Ok(mut g) = hops.lock() {
            g.push(format!("{} → {}", attempt.status(), attempt.url()));
        }
        attempt.follow()
    });
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .redirect(policy)
        .build()
        .map_err(|e| format!("HTTP 客户端构建失败: {}", e))
}

fn format_redirect_section(hops: &[String]) -> String {
    if hops.is_empty() {
        return "重定向: (无)\n".to_string();
    }
    let mut s = String::from("重定向:\n");
    for (i, line) in hops.iter().enumerate() {
        s.push_str(&format!("  {}. {}\n", i + 1, line));
    }
    s
}

/// 同步 GET 或 HEAD（阻塞）；HEAD 不读取正文，输出状态码、Content-Type、Content-Length 与重定向链。
pub fn fetch_with_method(
    url: &Url,
    method: FetchMethod,
    timeout_secs: u64,
    max_body_bytes: usize,
) -> String {
    let max_body_bytes = max_body_bytes.clamp(1024, ABS_MAX_BODY_BYTES);
    let hops: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let client = match build_client(timeout_secs, hops.clone()) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let req = match method {
        FetchMethod::Get => client.get(url.clone()),
        FetchMethod::Head => client.head(url.clone()),
    };

    let resp = match req.send() {
        Ok(r) => r,
        Err(e) => return format!("请求失败: {}", e),
    };

    let status = resp.status();
    let final_url = resp.url().clone();
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let clen = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let hop_lines = hops.lock().map(|g| g.clone()).unwrap_or_default();

    let mut out = String::new();
    out.push_str(&format!("method: {}\n", method.as_str()));
    out.push_str(&format!("请求 URL: {}\n", url));
    out.push_str(&format_redirect_section(&hop_lines));
    out.push_str(&format!("最终 URL: {}\n", final_url));
    out.push_str(&format!("状态: {}\n", status));
    out.push_str(&format!("Content-Type: {}\n", ctype));
    if clen.is_empty() {
        out.push_str("Content-Length: (未返回)\n");
    } else {
        out.push_str(&format!("Content-Length: {}\n", clen));
    }

    if method == FetchMethod::Head {
        out.push_str("\n(HEAD：未下载响应体)\n");
        return out.trim_end().to_string();
    }

    let bytes = match resp.bytes() {
        Ok(b) => b,
        Err(e) => return format!("读取响应体失败: {}", e),
    };
    let truncated = bytes.len() > max_body_bytes;
    let slice = if truncated {
        &bytes[..max_body_bytes]
    } else {
        &bytes[..]
    };
    let body_preview = String::from_utf8_lossy(slice);
    if truncated {
        out.push_str(&format!("\n正文已截断至前 {} 字节\n", max_body_bytes));
    } else {
        out.push('\n');
    }
    out.push_str("正文(UTF-8 有损预览):\n");
    out.push_str(&body_preview);
    if truncated {
        out.push_str("\n…");
    }
    out
}

/// 同步 HTTP 请求（POST/PUT/PATCH/DELETE + 可选 JSON body）。
pub fn request_with_json_body(
    url: &Url,
    method: RequestMethod,
    json_body: Option<&serde_json::Value>,
    timeout_secs: u64,
    max_body_bytes: usize,
) -> String {
    let max_body_bytes = max_body_bytes.clamp(1024, ABS_MAX_BODY_BYTES);
    let hops: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let client = match build_client(timeout_secs, hops.clone()) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let mut req = client.request(method.into_reqwest(), url.clone());
    let mut body_bytes = 0usize;
    if let Some(body) = json_body {
        match serde_json::to_vec(body) {
            Ok(v) => {
                body_bytes = v.len();
                req = req.header(CONTENT_TYPE, "application/json").body(v);
            }
            Err(e) => return format!("json_body 序列化失败: {}", e),
        }
    }
    let resp = match req.send() {
        Ok(r) => r,
        Err(e) => return format!("请求失败: {}", e),
    };

    let status = resp.status();
    let final_url = resp.url().clone();
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let clen = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let hop_lines = hops.lock().map(|g| g.clone()).unwrap_or_default();

    let mut out = String::new();
    out.push_str(&format!("method: {}\n", method.as_str()));
    out.push_str(&format!("请求 URL: {}\n", url));
    if json_body.is_some() {
        out.push_str(&format!("请求体: JSON（{} 字节）\n", body_bytes));
    } else {
        out.push_str("请求体: (无)\n");
    }
    out.push_str(&format_redirect_section(&hop_lines));
    out.push_str(&format!("最终 URL: {}\n", final_url));
    out.push_str(&format!("状态: {}\n", status));
    out.push_str(&format!("Content-Type: {}\n", ctype));
    if clen.is_empty() {
        out.push_str("Content-Length: (未返回)\n");
    } else {
        out.push_str(&format!("Content-Length: {}\n", clen));
    }

    let bytes = match resp.bytes() {
        Ok(b) => b,
        Err(e) => return format!("读取响应体失败: {}", e),
    };
    let truncated = bytes.len() > max_body_bytes;
    let slice = if truncated {
        &bytes[..max_body_bytes]
    } else {
        &bytes[..]
    };
    let body_preview = String::from_utf8_lossy(slice);
    if truncated {
        out.push_str(&format!("\n正文已截断至前 {} 字节\n", max_body_bytes));
    } else {
        out.push('\n');
    }
    out.push_str("正文(UTF-8 有损预览):\n");
    out.push_str(&body_preview);
    if truncated {
        out.push_str("\n…");
    }
    out
}
/// `run_tool` 同步路径：仅当 URL 匹配 `http_fetch_allowed_prefixes`（同源 + 路径前缀边界）时才请求；未匹配时返回错误（**不**在此路径弹审批；**repl/chat** 等经 `tool_registry` 异步路径可走 **`runtime::cli_approval`**）。
pub fn run_direct(args_json: &str, ctx: &ToolContext<'_>) -> String {
    let (url, method) = match parse_http_fetch_args(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：{}", e),
    };
    if !url_matches_allowed_prefixes(&url, ctx.http_fetch_allowed_prefixes) {
        return "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes（同源 + 路径前缀边界）。本同步路径仅允许白名单；Web 流式或 CLI（repl/chat）异步路径可人工审批。".to_string();
    }
    fetch_with_method(
        &url,
        method,
        ctx.http_fetch_timeout_secs,
        ctx.http_fetch_max_response_bytes,
    )
}

/// `http_request` 同步路径：仅匹配 `http_fetch_allowed_prefixes` 的 URL 才允许执行。
pub fn run_request_direct(args_json: &str, ctx: &ToolContext<'_>) -> String {
    let (url, method, json_body) = match parse_http_request_args(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：{}", e),
    };
    if !url_matches_allowed_prefixes(&url, ctx.http_fetch_allowed_prefixes) {
        return "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes（同源 + 路径前缀边界）；http_request 仅允许白名单前缀。".to_string();
    }
    request_with_json_body(
        &url,
        method,
        json_body.as_ref(),
        ctx.http_fetch_timeout_secs,
        ctx.http_fetch_max_response_bytes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_defaults_to_get() {
        let (u, m) = parse_http_fetch_args(r#"{"url":"https://example.com/a"}"#).unwrap();
        assert_eq!(m, FetchMethod::Get);
        assert_eq!(u.as_str(), "https://example.com/a");
    }

    #[test]
    fn parse_head() {
        let (u, m) =
            parse_http_fetch_args(r#"{"url":"https://example.com/","method":"head"}"#).unwrap();
        assert_eq!(m, FetchMethod::Head);
        assert_eq!(u.host_str(), Some("example.com"));
    }

    #[test]
    fn approval_display_head() {
        let u = Url::parse("https://ex.com/x?q=1").unwrap();
        let s = approval_args_display(FetchMethod::Head, &u);
        assert!(s.starts_with("HEAD "));
        assert!(s.contains("ex.com/x"));
        assert!(!s.contains("q=1"), "query 应被脱敏: {}", s);
    }

    #[test]
    fn allowed_prefix_requires_origin_and_path_boundary() {
        let url = Url::parse("https://example.com/api/v1/users").unwrap();
        assert!(url_matches_allowed_prefixes(
            &url,
            &["https://example.com/api/".to_string()]
        ));
        assert!(url_matches_allowed_prefixes(
            &url,
            &["https://example.com/api".to_string()]
        ));
        assert!(!url_matches_allowed_prefixes(
            &url,
            &["https://example.com/ap".to_string()]
        ));
        assert!(!url_matches_allowed_prefixes(
            &url,
            &["https://example.com/api2/".to_string()]
        ));
        assert!(!url_matches_allowed_prefixes(
            &url,
            &["https://example.comx/api/".to_string()]
        ));
    }
    #[test]
    fn parse_http_request_supports_patch_with_body() {
        let (u, m, body) = parse_http_request_args(
            r#"{"url":"https://example.com/api","method":"patch","json_body":{"x":1}}"#,
        )
        .unwrap();
        assert_eq!(u.as_str(), "https://example.com/api");
        assert_eq!(m, RequestMethod::Patch);
        let b = body.unwrap();
        assert_eq!(b.get("x").and_then(|x| x.as_i64()), Some(1));
    }

    #[test]
    fn parse_http_request_rejects_get() {
        let err =
            parse_http_request_args(r#"{"url":"https://example.com","method":"GET"}"#).unwrap_err();
        assert!(err.contains("POST/PUT/PATCH/DELETE"));
    }
}
