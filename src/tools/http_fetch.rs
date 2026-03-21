//! 受控 HTTP GET（仅 https/http）：用于拉取文档等；TUI 下未匹配配置前缀时需人工审批（与 `run_command` 同套 拒绝/一次/永久）。

use std::time::Duration;

use reqwest::Url;
use reqwest::redirect::Policy;

use super::ToolContext;

/// 响应体硬上限（与配置 `http_fetch_max_response_bytes` 上界一致）
pub const ABS_MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

pub fn parse_url_from_args(args_json: &str) -> Result<Url, String> {
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
    Ok(url)
}

/// 永久允许列表与审批判定的键（小写、无 query/fragment）
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

pub fn url_matches_allowed_prefixes(url: &str, prefixes: &[String]) -> bool {
    prefixes
        .iter()
        .any(|p| !p.trim().is_empty() && url.starts_with(p.trim()))
}

/// 同步 GET（阻塞）；用于 spawn_blocking 与「仅配置前缀」直连接径。
pub fn fetch_url(url: &Url, timeout_secs: u64, max_body_bytes: usize) -> String {
    let timeout_secs = timeout_secs.max(1);
    let max_body_bytes = max_body_bytes.clamp(1024, ABS_MAX_BODY_BYTES);
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .redirect(Policy::limited(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("HTTP 客户端构建失败: {}", e),
    };
    let resp = match client.get(url.clone()).send() {
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
    let mut out = String::new();
    out.push_str(&format!(
        "最终 URL: {}\n状态: {}\nContent-Type: {}\n",
        final_url,
        status,
        ctype.as_str()
    ));
    if truncated {
        out.push_str(&format!("正文已截断至前 {} 字节\n", max_body_bytes));
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
    let url = match parse_url_from_args(args_json) {
        Ok(u) => u,
        Err(e) => return format!("错误：{}", e),
    };
    let url_str = url.as_str();
    if !url_matches_allowed_prefixes(url_str, ctx.http_fetch_allowed_prefixes) {
        return "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes。Web 模式仅允许白名单前缀；TUI 下可对单次请求使用审批（同意/拒绝/永久同意）。".to_string();
    }
    fetch_url(
        &url,
        ctx.http_fetch_timeout_secs,
        ctx.http_fetch_max_response_bytes,
    )
}
