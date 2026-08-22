use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest::redirect::Policy;

use super::super::ToolContext;
use super::args::{
    ABS_MAX_BODY_BYTES, FetchMethod, HttpBodyTextFormat, RequestMethod, parse_http_fetch_args,
    parse_http_request_args,
};
use super::decode::{apply_text_format_if_requested, decode_http_body_text_for_tool};
use super::policy::url_matches_allowed_prefixes;

/// 默认 UA：`crabmate/<版本>`；可经配置 `http_fetch_user_agent` / `CM_HTTP_FETCH_USER_AGENT` 覆盖。
pub fn default_user_agent() -> String {
    format!("crabmate/{}", env!("CARGO_PKG_VERSION"))
}

/// 从 `ToolContext.cfg`（若有）解析已配置的 UA；缺省回落到 [`default_user_agent`]。
fn resolve_user_agent(cfg: Option<&crate::cm_config::AgentConfig>) -> String {
    cfg.map(|c| c.http_fetch.http_fetch_user_agent.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(default_user_agent)
}

fn build_client(
    timeout_secs: u64,
    redirect_hops: Arc<Mutex<Vec<String>>>,
    user_agent: &str,
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
    // 与 curl 默认对齐：显式声明 `Accept: */*`（reqwest 默认不发送该头）。
    let mut default_headers = HeaderMap::new();
    default_headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .redirect(policy)
        .default_headers(default_headers)
        .user_agent(user_agent)
        .build()
        .map_err(|e| format!("HTTP 客户端构建失败: {}", e))
}

/// 格式化 reqwest 错误并附带 `source()` 链（如代理连接失败的具体原因），便于诊断代理/网络问题。
/// 检测到 **SOCKS 环境代理**（`ALL_PROXY`/`HTTP_PROXY`/`HTTPS_PROXY` 为 `socks5://` 等）时追加可操作提示：
/// reqwest 0.13 仅支持 HTTP(S) 代理，SOCKS 需 curl 等原生客户端。
fn format_reqwest_error(e: &reqwest::Error) -> String {
    let mut s = format!("请求失败: {}", e);
    let mut chain = String::new();
    let mut src = e.source();
    while let Some(c) = src {
        chain.push_str(&format!(" → {}", c));
        src = c.source();
    }
    if chain.contains("unsupported scheme socks") {
        s.push_str(
            "；检测到 SOCKS 环境代理，但 reqwest 0.13 不支持 socks 代理：可 unset ALL_PROXY/HTTPS_PROXY/HTTP_PROXY，或用同端口的 http:// 代理（如 Clash 混合端口）重试",
        );
    }
    s.push_str(&chain);
    s
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
    url: &reqwest::Url,
    method: FetchMethod,
    text_format: HttpBodyTextFormat,
    user_agent: &str,
    timeout_secs: u64,
    max_body_bytes: usize,
) -> String {
    let max_body_bytes = max_body_bytes.clamp(1024, ABS_MAX_BODY_BYTES);
    let hops: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let client = match build_client(timeout_secs, hops.clone(), user_agent) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let req = match method {
        FetchMethod::Get => client.get(url.clone()),
        FetchMethod::Head => client.head(url.clone()),
    };

    let resp = match req.send() {
        Ok(r) => r,
        Err(e) => return format_reqwest_error(&e),
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
    let (decoded, decode_note) = decode_http_body_text_for_tool(&ctype, slice);
    let (body_preview, decode_note) =
        apply_text_format_if_requested(&ctype, text_format, decoded, decode_note);
    if truncated {
        out.push_str(&format!("\n正文已截断至前 {} 字节\n", max_body_bytes));
    } else {
        out.push('\n');
    }
    out.push_str(&decode_note);
    out.push('\n');
    out.push_str("正文:\n");
    out.push_str(&body_preview);
    if truncated {
        out.push_str("\n…");
    }
    out
}

fn attach_optional_json_body(
    req: reqwest::blocking::RequestBuilder,
    json_body: Option<&serde_json::Value>,
) -> Result<(reqwest::blocking::RequestBuilder, usize), String> {
    let Some(body) = json_body else {
        return Ok((req, 0));
    };
    let v = serde_json::to_vec(body).map_err(|e| format!("json_body 序列化失败: {}", e))?;
    let n = v.len();
    Ok((req.header(CONTENT_TYPE, "application/json").body(v), n))
}

fn append_json_body_line(out: &mut String, json_body: bool, body_bytes: usize) {
    if json_body {
        out.push_str(&format!("请求体: JSON（{} 字节）\n", body_bytes));
    } else {
        out.push_str("请求体: (无)\n");
    }
}

fn append_content_length_line(out: &mut String, clen: &str) {
    if clen.is_empty() {
        out.push_str("Content-Length: (未返回)\n");
    } else {
        out.push_str(&format!("Content-Length: {}\n", clen));
    }
}

/// 同步 HTTP 请求（POST/PUT/PATCH/DELETE + 可选 JSON body）。
pub fn request_with_json_body(
    url: &reqwest::Url,
    method: RequestMethod,
    json_body: Option<&serde_json::Value>,
    text_format: HttpBodyTextFormat,
    user_agent: &str,
    timeout_secs: u64,
    max_body_bytes: usize,
) -> String {
    let max_body_bytes = max_body_bytes.clamp(1024, ABS_MAX_BODY_BYTES);
    let hops: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let client = match build_client(timeout_secs, hops.clone(), user_agent) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let req = client.request(method.into_reqwest(), url.clone());
    let (req, body_bytes) = match attach_optional_json_body(req, json_body) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let resp = match req.send() {
        Ok(r) => r,
        Err(e) => return format_reqwest_error(&e),
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
    append_json_body_line(&mut out, json_body.is_some(), body_bytes);
    out.push_str(&format_redirect_section(&hop_lines));
    out.push_str(&format!("最终 URL: {}\n", final_url));
    out.push_str(&format!("状态: {}\n", status));
    out.push_str(&format!("Content-Type: {}\n", ctype));
    append_content_length_line(&mut out, clen);

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
    let (decoded, decode_note) = decode_http_body_text_for_tool(&ctype, slice);
    let (body_preview, decode_note) =
        apply_text_format_if_requested(&ctype, text_format, decoded, decode_note);
    if truncated {
        out.push_str(&format!("\n正文已截断至前 {} 字节\n", max_body_bytes));
    } else {
        out.push('\n');
    }
    out.push_str(&decode_note);
    out.push('\n');
    out.push_str("正文:\n");
    out.push_str(&body_preview);
    if truncated {
        out.push_str("\n…");
    }
    out
}

/// `run_tool` 同步路径：仅当 URL 匹配 `http_fetch_allowed_prefixes`（同源 + 路径前缀边界）时才请求；未匹配时返回错误（**不**在此路径弹审批；CLI 经 `tool_registry` 异步路径可走 **`tool_approval`**）。
pub fn run_direct(args_json: &str, ctx: &ToolContext<'_>) -> String {
    let (url, method, text_format) = match parse_http_fetch_args(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：{}", e),
    };
    if !url_matches_allowed_prefixes(&url, ctx.http_fetch_allowed_prefixes) {
        return "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes（同源 + 路径前缀边界）。本同步路径仅允许白名单；Web 流式或 CLI 异步路径可人工审批。".to_string();
    }
    let user_agent = resolve_user_agent(ctx.cfg);
    fetch_with_method(
        &url,
        method,
        text_format,
        &user_agent,
        ctx.http_fetch_timeout_secs,
        ctx.http_fetch_max_response_bytes,
    )
}

/// `http_request` 同步路径：仅匹配 `http_fetch_allowed_prefixes` 的 URL 才允许执行。
pub fn run_request_direct(args_json: &str, ctx: &ToolContext<'_>) -> String {
    let (url, method, json_body, text_format) = match parse_http_request_args(args_json) {
        Ok(x) => x,
        Err(e) => return format!("错误：{}", e),
    };
    if !url_matches_allowed_prefixes(&url, ctx.http_fetch_allowed_prefixes) {
        return "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes（同源 + 路径前缀边界）。本同步路径仅允许白名单；Web 流式或 CLI 异步路径可对 http_request 人工审批。".to_string();
    }
    let user_agent = resolve_user_agent(ctx.cfg);
    request_with_json_body(
        &url,
        method,
        json_body.as_ref(),
        text_format,
        &user_agent,
        ctx.http_fetch_timeout_secs,
        ctx.http_fetch_max_response_bytes,
    )
}
