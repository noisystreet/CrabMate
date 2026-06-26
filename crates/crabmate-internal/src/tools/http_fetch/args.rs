use reqwest::Url;
use schemars::JsonSchema;
use serde::Deserialize;

/// `http_fetch` 工具入参（与发给模型的 `parameters` 同源，见 `tool_params::params_http_fetch`）。
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HttpFetchArgs {
    /// 完整 http(s) URL
    pub url: String,
    /// `GET` / `HEAD`（大小写均可），默认 `GET`
    pub method: Option<String>,
    /// `raw`（默认）或 `html_text` 等
    pub text_format: Option<String>,
}

/// `http_request` 工具入参。
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HttpRequestArgs {
    pub url: String,
    pub method: String,
    /// 可选 JSON 请求体
    pub json_body: Option<serde_json::Value>,
    pub text_format: Option<String>,
}

/// 响应体硬上限（与配置 `http_fetch_max_response_bytes` 上界一致）
pub const ABS_MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// `http_fetch` / `http_request` 工具：正文呈现方式（默认保留解码后的原文）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HttpBodyTextFormat {
    /// 解码后的字符串原样输出（可能含 HTML 标签）。
    #[default]
    Raw,
    /// 将 HTML 转为纯文本（非 HTML 或解析失败时保留原文并在说明中提示）。
    HtmlText,
}

pub(super) fn parse_text_format_optional(raw: Option<&str>) -> Result<HttpBodyTextFormat, String> {
    let raw = raw
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("raw");
    let n = raw.to_ascii_lowercase().replace('-', "_");
    match n.as_str() {
        "raw" => Ok(HttpBodyTextFormat::Raw),
        "html_text" | "htmltext" | "text" => Ok(HttpBodyTextFormat::HtmlText),
        _ => Err(format!(
            "text_format 仅支持 raw（默认）或 html_text（收到 {:?}）",
            raw
        )),
    }
}

/// `http_request` JSON 请求体上限（字节，序列化后）
const MAX_REQUEST_JSON_BODY_BYTES: usize = 256 * 1024;

/// 与 `http_fetch` 工具对应的 HTTP 方法（默认 GET）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchMethod {
    Get,
    Head,
}

impl FetchMethod {
    pub fn as_str(self) -> &'static str {
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
    pub fn as_str(self) -> &'static str {
        match self {
            RequestMethod::Post => "POST",
            RequestMethod::Put => "PUT",
            RequestMethod::Patch => "PATCH",
            RequestMethod::Delete => "DELETE",
        }
    }

    pub fn into_reqwest(self) -> reqwest::Method {
        match self {
            RequestMethod::Post => reqwest::Method::POST,
            RequestMethod::Put => reqwest::Method::PUT,
            RequestMethod::Patch => reqwest::Method::PATCH,
            RequestMethod::Delete => reqwest::Method::DELETE,
        }
    }
}

/// 解析 `url`、可选 `method`（`GET` / `HEAD`，默认 `GET`）与可选 **`text_format`**。
pub fn parse_http_fetch_args(
    args_json: &str,
) -> Result<(Url, FetchMethod, HttpBodyTextFormat), String> {
    let args: HttpFetchArgs =
        serde_json::from_str(args_json).map_err(|e| format!("参数 JSON 无效: {e}"))?;
    let u = args.url.trim();
    if u.is_empty() {
        return Err("缺少 url".to_string());
    }
    let url = Url::parse(u).map_err(|e| format!("URL 解析失败: {}", e))?;
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!("仅允许 http/https 方案，当前为 {}", scheme));
    }

    let method_upper = args
        .method
        .as_deref()
        .map(|s| s.trim().to_ascii_uppercase())
        .filter(|s| !s.is_empty());

    let method = match method_upper.as_deref() {
        None | Some("GET") => FetchMethod::Get,
        Some("HEAD") => FetchMethod::Head,
        Some(other) => {
            return Err(format!("method 仅支持 GET 或 HEAD（收到 {:?}）", other));
        }
    };

    let text_format = parse_text_format_optional(args.text_format.as_deref())?;
    Ok((url, method, text_format))
}

/// 解析 `http_request` 入参：`url` + `method`（POST/PUT/PATCH/DELETE）+ 可选 `json_body` + 可选 **`text_format`**。
pub fn parse_http_request_args(
    args_json: &str,
) -> Result<
    (
        Url,
        RequestMethod,
        Option<serde_json::Value>,
        HttpBodyTextFormat,
    ),
    String,
> {
    let args: HttpRequestArgs =
        serde_json::from_str(args_json).map_err(|e| format!("参数 JSON 无效: {e}"))?;
    let u = args.url.trim();
    if u.is_empty() {
        return Err("缺少 url".to_string());
    }
    let url = Url::parse(u).map_err(|e| format!("URL 解析失败: {}", e))?;
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!("仅允许 http/https 方案，当前为 {}", scheme));
    }
    let method_raw = args.method.trim().to_ascii_uppercase();
    if method_raw.is_empty() {
        return Err("缺少 method（POST/PUT/PATCH/DELETE）".to_string());
    }
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
    let json_body = args.json_body.clone();
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
    let text_format = parse_text_format_optional(args.text_format.as_deref())?;
    Ok((url, method, json_body, text_format))
}
