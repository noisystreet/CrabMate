//! 受控 HTTP GET / HEAD（仅 https/http）；TUI 下未匹配配置前缀时需人工审批（与 `run_command` 同套 拒绝/一次/永久同意）。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::Url;
use reqwest::redirect::Policy;

use super::ToolContext;

/// 响应体硬上限（与配置 `http_fetch_max_response_bytes` 上界一致）
pub const ABS_MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

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

pub fn url_matches_allowed_prefixes(url: &str, prefixes: &[String]) -> bool {
    prefixes
        .iter()
        .any(|p| !p.trim().is_empty() && url.starts_with(p.trim()))
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

/// `run_tool` 同步路径：仅当 URL 以配置的 `http_fetch_allowed_prefixes` 之一为前缀时才请求；否则提示配置或 TUI。
pub fn run_direct(args_json: &str, ctx: &ToolContext<'_>) -> String {
    let (url, method) = match parse_http_fetch_args(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：{}", e),
    };
    let url_str = url.as_str();
    if !url_matches_allowed_prefixes(url_str, ctx.http_fetch_allowed_prefixes) {
        return "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes。Web 模式仅允许白名单前缀；TUI 下可对单次请求使用审批（同意/拒绝/永久同意）。".to_string();
    }
    fetch_with_method(
        &url,
        method,
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
}
