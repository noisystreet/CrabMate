//! OpenAI 兼容 **`GET /v1/models`**（或 `api_base` 下的 `models`）探测与列表解析。
//!
//! 响应体与错误信息**不得**写入日志或终端全文（避免泄露供应商返回中的敏感片段）；仅输出状态码、耗时与模型 id 列表。

use std::time::Instant;

use reqwest::Client;
use serde::Deserialize;

use crate::config::LlmHttpAuthMode;
use crate::http_client;
use crate::types::OPENAI_MODELS_REL_PATH;

#[derive(Debug, Deserialize)]
struct ModelsEnvelope {
    #[serde(default)]
    data: Option<Vec<ModelRow>>,
}

#[derive(Debug, Deserialize)]
struct ModelRow {
    id: String,
}

/// `GET …/models` 的结果摘要（供 `crabmate models` / `crabmate probe`）。
#[derive(Debug, Clone)]
pub struct ModelsEndpointReport {
    pub url_display: String,
    pub http_status: u16,
    pub elapsed_ms: u128,
    pub model_ids: Vec<String>,
    /// 非成功 HTTP 或 JSON 形不对时的简短说明（不含响应体原文）。
    pub note: Option<String>,
}

fn models_url(api_base: &str) -> String {
    format!(
        "{}/{}",
        api_base.trim_end_matches('/'),
        OPENAI_MODELS_REL_PATH
    )
}

/// 请求 `api_base/models`，解析 `data[].id`（OpenAI 兼容形）。
pub async fn fetch_models_report(
    client: &Client,
    api_base: &str,
    api_key: &str,
    auth_mode: LlmHttpAuthMode,
) -> Result<ModelsEndpointReport, Box<dyn std::error::Error + Send + Sync>> {
    let url = models_url(api_base);
    let url_display = redact_url_for_display(&url);
    let t0 = Instant::now();
    let mut rb = client.get(&url);
    if auth_mode == LlmHttpAuthMode::Bearer {
        rb = rb.header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", api_key.trim()),
        );
    }
    let resp = rb
        .send()
        .await
        .map_err(http_client::map_reqwest_transport_err)?;
    let elapsed_ms = t0.elapsed().as_millis();
    let http_status = resp.status().as_u16();
    let body = resp
        .text()
        .await
        .map_err(http_client::map_reqwest_transport_err)?;

    if !(200..300).contains(&http_status) {
        return Ok(ModelsEndpointReport {
            url_display,
            http_status,
            elapsed_ms,
            model_ids: Vec::new(),
            note: Some(format!(
                "HTTP {http_status}（响应体已省略，请检查 API_KEY 与 api_base）"
            )),
        });
    }

    match serde_json::from_str::<ModelsEnvelope>(&body) {
        Ok(env) => {
            let mut model_ids: Vec<String> = env
                .data
                .unwrap_or_default()
                .into_iter()
                .map(|r| r.id)
                .collect();
            model_ids.sort();
            model_ids.dedup();
            let note = if model_ids.is_empty() {
                Some("HTTP 成功但 data 为空或缺 id 字段（网关可能非标准形）".to_string())
            } else {
                None
            };
            Ok(ModelsEndpointReport {
                url_display,
                http_status,
                elapsed_ms,
                model_ids,
                note,
            })
        }
        Err(e) => Ok(ModelsEndpointReport {
            url_display,
            http_status,
            elapsed_ms,
            model_ids: Vec::new(),
            note: Some(format!(
                "JSON 与 OpenAI models 形不一致（不打印响应体）: {e}"
            )),
        }),
    }
}

/// 若 URL 带 query，终端展示时整段 query 折叠为 `?…`，避免误把令牌打在屏幕上。
fn redact_url_for_display(url: &str) -> String {
    match url.split_once('?') {
        Some((base, q)) if !q.is_empty() => format!("{base}?…"),
        _ => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::redact_url_for_display;

    #[test]
    fn redact_query_when_present() {
        let u = "https://example.com/v1/models?key=sk-xx&foo=1";
        let d = redact_url_for_display(u);
        assert!(!d.contains("sk-xx"));
        assert!(d.ends_with("?…"));
    }
}
