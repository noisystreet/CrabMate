//! 联网网页搜索：通过第三方 HTTP API（Brave / Tavily）查询，使用 reqwest + serde。

use crate::config::WebSearchProvider;
use crate::redact::{self, HTTP_BODY_PREVIEW_LOG_CHARS};
use serde::Deserialize;
use tracing::warn;

use super::ToolContext;

const BRAVE_SEARCH_URL: &str = "https://api.search.brave.com/res/v1/web/search";
const TAVILY_SEARCH_URL: &str = "https://api.tavily.com/search";

#[derive(Debug, Deserialize)]
struct BraveWebSearchResponse {
    web: Option<BraveWeb>,
}

#[derive(Debug, Deserialize)]
struct BraveWeb {
    results: Option<Vec<BraveWebResult>>,
}

#[derive(Debug, Deserialize)]
struct BraveWebResult {
    title: Option<String>,
    url: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TavilySearchResponse {
    results: Option<Vec<TavilyResult>>,
}

#[derive(Debug, Deserialize)]
struct TavilyResult {
    title: Option<String>,
    url: Option<String>,
    content: Option<String>,
}

/// 执行 `web_search` 工具：参数 JSON 含 `query`，可选 `max_results`（1～20）。
pub fn run(args_json: &str, ctx: &ToolContext<'_>) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("错误：参数 JSON 无效：{}", e),
    };
    let query = match v.get("query").and_then(|q| q.as_str()).map(str::trim) {
        Some(s) if s.len() >= 2 => s.to_string(),
        Some(_) => return "错误：query 至少 2 个字符".to_string(),
        None => return "错误：缺少 query 参数".to_string(),
    };

    let max_results = v
        .get("max_results")
        .and_then(|n| n.as_u64())
        .map(|n| n.clamp(1, 20) as u32)
        .unwrap_or(ctx.web_search_max_results)
        .clamp(1, 20);

    if ctx.web_search_api_key.trim().is_empty() {
        return "错误：未配置联网搜索 API Key。请在配置中设置 web_search_api_key，或设置环境变量 AGENT_WEB_SEARCH_API_KEY；并设置 web_search_provider 为 brave 或 tavily（参见 README）。".to_string();
    }

    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(ctx.web_search_timeout_secs))
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("HTTP 客户端创建失败：{}", e),
    };

    let raw = match ctx.web_search_provider {
        WebSearchProvider::Brave => {
            search_brave(&client, ctx.web_search_api_key, &query, max_results)
        }
        WebSearchProvider::Tavily => {
            search_tavily(&client, ctx.web_search_api_key, &query, max_results)
        }
    };

    let raw = match raw {
        Ok(s) => s,
        Err(e) => return e,
    };

    truncate_output(&raw, ctx.command_max_output_len)
}

fn search_brave(
    client: &reqwest::blocking::Client,
    api_key: &str,
    query: &str,
    max_results: u32,
) -> Result<String, String> {
    let res = client
        .get(BRAVE_SEARCH_URL)
        .header("X-Subscription-Token", api_key.trim())
        .header("Accept", "application/json")
        .query(&[("q", query), ("count", &max_results.to_string())])
        .send()
        .map_err(|e| format!("Brave 搜索请求失败：{}", e))?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().unwrap_or_default();
        let preview = redact::single_line_preview(&body, HTTP_BODY_PREVIEW_LOG_CHARS);
        warn!(
            provider = "brave",
            status = %status,
            body_len = body.len(),
            body_preview = %preview,
            "Brave 搜索 API 非成功响应"
        );
        return Err(format!(
            "Brave 搜索 API 返回错误（HTTP {}），请检查 API 密钥或稍后重试",
            status.as_u16()
        ));
    }

    let parsed: BraveWebSearchResponse = res
        .json()
        .map_err(|e| format!("解析 Brave 响应失败：{}", e))?;

    let results = parsed.web.and_then(|w| w.results).unwrap_or_default();

    if results.is_empty() {
        return Ok("（无网页结果）".to_string());
    }

    let mut out = String::from("联网搜索（Brave）结果：\n\n");
    for (i, r) in results.iter().enumerate() {
        let title = r.title.as_deref().unwrap_or("(无标题)");
        let url = r.url.as_deref().unwrap_or("");
        let desc = r.description.as_deref().unwrap_or("");
        out.push_str(&format!(
            "{}. {}\n   URL: {}\n   {}\n\n",
            i + 1,
            title,
            url,
            desc.trim()
        ));
    }
    Ok(out.trim_end().to_string())
}

fn search_tavily(
    client: &reqwest::blocking::Client,
    api_key: &str,
    query: &str,
    max_results: u32,
) -> Result<String, String> {
    let body = serde_json::json!({
        "api_key": api_key.trim(),
        "query": query,
        "max_results": max_results,
        "search_depth": "basic",
    });

    let res = client
        .post(TAVILY_SEARCH_URL)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| format!("Tavily 搜索请求失败：{}", e))?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().unwrap_or_default();
        let preview = redact::single_line_preview(&text, HTTP_BODY_PREVIEW_LOG_CHARS);
        warn!(
            provider = "tavily",
            status = %status,
            body_len = text.len(),
            body_preview = %preview,
            "Tavily 搜索 API 非成功响应"
        );
        return Err(format!(
            "Tavily 搜索 API 返回错误（HTTP {}），请检查 API 密钥或稍后重试",
            status.as_u16()
        ));
    }

    let parsed: TavilySearchResponse = res
        .json()
        .map_err(|e| format!("解析 Tavily 响应失败：{}", e))?;

    let results = parsed.results.unwrap_or_default();
    if results.is_empty() {
        return Ok("（无网页结果）".to_string());
    }

    let mut out = String::from("联网搜索（Tavily）结果：\n\n");
    for (i, r) in results.iter().enumerate() {
        let title = r.title.as_deref().unwrap_or("(无标题)");
        let url = r.url.as_deref().unwrap_or("");
        let content = r.content.as_deref().unwrap_or("");
        out.push_str(&format!(
            "{}. {}\n   URL: {}\n   {}\n\n",
            i + 1,
            title,
            url,
            content.trim()
        ));
    }
    Ok(out.trim_end().to_string())
}

fn truncate_output(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max_chars.saturating_sub(80)).collect();
    t.push_str("\n\n…（输出已按 command_max_output_len 截断）");
    t
}
