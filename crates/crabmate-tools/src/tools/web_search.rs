//! 联网网页搜索：默认经 [worbrow](https://crates.io/crates/worbrow) 驱动本机浏览器；
//! 亦可选用 Brave / Tavily HTTP API（需 API Key）。

use std::time::Duration;

use crate::redact::{self, HTTP_BODY_PREVIEW_LOG_CHARS};
use crabmate_config::WebSearchProvider;
use log::warn;
use serde::Deserialize;
use worbrow::{BrowserKind, Config as WorbrowConfig, DoctorReport, Outcome, search};

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
    let args: super::tool_param_types::WebSearchArgs = match serde_json::from_str(args_json) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 无效: {e}"),
    };
    let query = args.query.trim();
    let query = if query.len() >= 2 {
        query.to_string()
    } else if query.is_empty() {
        return "错误：缺少 query 参数".to_string();
    } else {
        return "错误：query 至少 2 个字符".to_string();
    };

    let max_results = args
        .max_results
        .map(|n| n.clamp(1, 20) as u32)
        .unwrap_or(ctx.web_search_max_results)
        .clamp(1, 20);

    let raw = match ctx.web_search_provider {
        WebSearchProvider::Worbrow => {
            search_worbrow(&query, max_results, ctx.web_search_timeout_secs)
        }
        WebSearchProvider::Brave | WebSearchProvider::Tavily => {
            if ctx.web_search_api_key.trim().is_empty() {
                return "错误：未配置联网搜索 API Key。请在配置中设置 web_search_api_key，或设置环境变量 CM_WEB_SEARCH_API_KEY；并设置 web_search_provider 为 brave 或 tavily。若希望免 Key，请使用默认的 worbrow（本机浏览器）或将 web_search_provider 设为 worbrow。".to_string();
            }
            let client = match reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(ctx.web_search_timeout_secs))
                .build()
            {
                Ok(c) => c,
                Err(e) => return format!("HTTP 客户端创建失败：{}", e),
            };
            match ctx.web_search_provider {
                WebSearchProvider::Brave => {
                    search_brave(&client, ctx.web_search_api_key, &query, max_results)
                }
                WebSearchProvider::Tavily => {
                    search_tavily(&client, ctx.web_search_api_key, &query, max_results)
                }
                WebSearchProvider::Worbrow => unreachable!("worbrow 已在上方分支处理"),
            }
        }
    };

    let raw = match raw {
        Ok(s) => s,
        Err(e) => return e,
    };

    truncate_output(&raw, ctx.command_max_output_len)
}

/// 首选 Bing，验证码/低产时降级 DuckDuckGo（worbrow 0.1.1 引擎链）。
const WORBROW_ENGINE_CHAIN: &str = "bing,duckduckgo";

fn search_worbrow(query: &str, max_results: u32, timeout_secs: u64) -> Result<String, String> {
    let browser = resolve_worbrow_browser()?;
    let cfg = WorbrowConfig::new(query, WORBROW_ENGINE_CHAIN, browser)
        .with_max_results(max_results as usize)
        .with_timeout(Duration::from_secs(timeout_secs.max(1)));
    let outcome = search(cfg).map_err(|e| format_worbrow_error(&e))?;
    Ok(format_worbrow_outcome(&outcome, browser))
}

/// 优先 Firefox（与 worbrow 默认一致），其次 Chrome/Edge/Chromium。
fn resolve_worbrow_browser() -> Result<BrowserKind, String> {
    let report = DoctorReport::collect();
    for preferred in [BrowserKind::Firefox, BrowserKind::Chrome] {
        if report
            .backends
            .iter()
            .any(|b| b.kind == preferred && b.binary.is_some())
        {
            return Ok(preferred);
        }
    }
    Err(
        "错误：未找到可用的本机浏览器（需 Firefox，或 Chrome/Edge/Chromium）。\
请安装浏览器后重试，或将 web_search_provider 设为 brave/tavily 并配置 web_search_api_key / CM_WEB_SEARCH_API_KEY。"
            .to_string(),
    )
}

fn format_worbrow_error(err: &worbrow::Error) -> String {
    format!(
        "worbrow 搜索失败（{}）：{}。若环境无浏览器或遇验证码，可改用 brave/tavily 并配置 API Key。",
        err.code_str(),
        err
    )
}

fn format_worbrow_outcome(outcome: &Outcome, browser: BrowserKind) -> String {
    if outcome.results.is_empty() {
        let mut s = String::from("（无网页结果）");
        append_worbrow_meta_notes(&mut s, outcome);
        return s;
    }

    let mut out = format!(
        "联网搜索（worbrow / {} / {}）结果：\n\n",
        outcome.meta.engine, browser
    );
    for r in &outcome.results {
        let scheme = if r.https { "https" } else { "http" };
        let domain = if r.domain.is_empty() {
            String::new()
        } else {
            format!(" ({scheme}://{})", r.domain)
        };
        out.push_str(&format!(
            "{}. {}{}\n   URL: {}\n   {}\n\n",
            r.rank,
            r.title,
            domain,
            r.url,
            r.snippet.trim()
        ));
    }
    let mut s = out.trim_end().to_string();
    append_worbrow_meta_notes(&mut s, outcome);
    s
}

fn append_worbrow_meta_notes(out: &mut String, outcome: &Outcome) {
    let mut notes = Vec::new();
    if outcome.meta.engine_tried.len() > 1 {
        notes.push(format!(
            "引擎尝试链：{}",
            outcome.meta.engine_tried.join(" → ")
        ));
    }
    if outcome.meta.captcha {
        notes.push("检测到验证码，结果可能不完整".to_string());
    }
    if outcome.meta.low_yield {
        notes.push("结果条数偏少（low_yield）".to_string());
    }
    if let Some(ref e) = outcome.meta.engine_error {
        notes.push("引擎侧异常".to_string());
        warn!(
            target: "crabmate",
            "worbrow engine_error code={} message_len={}",
            e.code,
            e.message.len()
        );
    }
    if notes.is_empty() {
        return;
    }
    out.push_str("\n\n注意：");
    out.push_str(&notes.join("；"));
    out.push('。');
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
            target: "crabmate",
            "Brave 搜索 API 非成功响应 provider=brave status={} body_len={} body_preview={}",
            status,
            body.len(),
            preview
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
            target: "crabmate",
            "Tavily 搜索 API 非成功响应 provider=tavily status={} body_len={} body_preview={}",
            status,
            text.len(),
            preview
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

#[cfg(test)]
mod tests {
    use super::*;
    use worbrow::{BrowserKind, Config as WorbrowConfig, search};

    #[test]
    fn worbrow_fake_formats_non_empty_results() {
        let outcome = search(WorbrowConfig::new("rust", "bing", BrowserKind::Fake))
            .expect("fake backend should succeed offline");
        let text = format_worbrow_outcome(&outcome, BrowserKind::Fake);
        assert!(text.contains("联网搜索（worbrow"));
        assert!(text.contains("URL:"));
    }

    #[test]
    fn provider_parse_accepts_worbrow_aliases() {
        assert_eq!(
            WebSearchProvider::parse("worbrow").unwrap(),
            WebSearchProvider::Worbrow
        );
        assert_eq!(
            WebSearchProvider::parse("browser").unwrap(),
            WebSearchProvider::Worbrow
        );
        assert!(!WebSearchProvider::default().requires_api_key());
        assert!(WebSearchProvider::Brave.requires_api_key());
    }

    /// 实网：本机 Firefox/Chrome + Bing。默认忽略；本地：  
    /// `cargo test -p crabmate-tools live_worbrow_search -- --ignored --nocapture`
    #[test]
    #[ignore = "requires local browser + network"]
    fn live_worbrow_search() {
        let out = search_worbrow("Rust async tokio", 5, 90).expect("worbrow search");
        eprintln!("{out}");
        assert!(
            out.contains("URL:") || out.contains("无网页结果"),
            "unexpected output: {out}"
        );
        assert!(!out.contains("搜索失败"), "search reported failure: {out}");
    }
}
