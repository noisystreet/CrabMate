//! 请求体验证与规范化（conversation_id、client_llm、seed、temperature 等）。

use axum::Json;
use axum::http::StatusCode;

use super::super::app_state::{AppState, CONVERSATION_ID_MAX_LEN};
use crate::chat_job_queue::{self, WebClientLlmThinkingMode};
use crate::config::LlmHttpAuthMode;
use crate::web::http_types::chat::{ApiError, ClientLlmBody, ExecutorLlmBody};

/// Web 聊天附带的图片 URL：仅允许同源 **`/uploads/<文件名>`**（与 `upload_handler` 一致），防目录穿越与外链滥用。
pub(super) fn normalize_chat_image_urls(raw: &[String]) -> Result<Vec<String>, String> {
    const MAX_IMAGES: usize = 6;
    if raw.len() > MAX_IMAGES {
        return Err(format!("图片数量过多（上限 {MAX_IMAGES} 张）"));
    }
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    for u in raw {
        let t = u.trim();
        if t.is_empty() {
            continue;
        }
        if t.contains("..") || t.contains('\\') || t.starts_with("//") {
            return Err("图片 URL 非法".to_string());
        }
        if !t.starts_with("/uploads/") {
            return Err("图片 URL 须为 /uploads/ 下的相对路径".to_string());
        }
        let name = t.trim_start_matches("/uploads/");
        if name.is_empty() || name.contains('/') {
            return Err("图片 URL 非法".to_string());
        }
        out.push(format!("/uploads/{name}"));
    }
    Ok(out)
}

const CLIENT_LLM_API_BASE_MAX: usize = 2048;
const CLIENT_LLM_MODEL_MAX: usize = 512;
const CLIENT_LLM_API_KEY_MAX: usize = 16384;

fn validate_web_override_api_base(raw: &str) -> Result<(), String> {
    let t = raw.trim();
    if t.is_empty() {
        return Err("api_base 不能为空".to_string());
    }
    let u = reqwest::Url::parse(t).map_err(|_| "api_base URL 格式无效".to_string())?;
    match u.scheme() {
        "http" | "https" => {}
        _ => return Err("api_base 仅支持 http 或 https".to_string()),
    }
    let host = u
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| "api_base 须包含主机名".to_string())?;
    let host_lc = host.to_ascii_lowercase();
    if host_lc == "169.254.169.254"
        || host_lc == "metadata.google.internal"
        || host_lc == "metadata.goog"
    {
        return Err("api_base 主机不被允许".to_string());
    }
    Ok(())
}

fn validate_optional_web_api_base(
    api_base: &Option<String>,
    prefix: &'static str,
) -> Result<(), String> {
    let Some(s) = api_base else {
        return Ok(());
    };
    validate_web_override_api_base(s)?;
    if s.len() > CLIENT_LLM_API_BASE_MAX {
        return Err(format!(
            "{prefix}.api_base 过长（上限 {} 字符）",
            CLIENT_LLM_API_BASE_MAX
        ));
    }
    Ok(())
}

fn validate_optional_field_max_len(
    value: &Option<String>,
    max: usize,
    prefix: &'static str,
    field: &'static str,
) -> Result<(), String> {
    if let Some(s) = value
        && s.len() > max
    {
        return Err(format!("{prefix}.{field} 过长（上限 {max} 字符）",));
    }
    Ok(())
}

pub(super) fn parse_client_llm_override(
    raw: Option<ClientLlmBody>,
) -> Result<Option<chat_job_queue::WebChatLlmOverride>, String> {
    let Some(b) = raw else {
        return Ok(None);
    };
    let api_base = b
        .api_base
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let model = b
        .model
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let api_key = b
        .api_key
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let llm_context_tokens = match b.llm_context_tokens {
        None => None,
        Some(0) => None,
        Some(n) => {
            if n > 10_000_000 {
                return Err("client_llm.llm_context_tokens 过大（上限 10000000）".to_string());
            }
            Some(n as u32)
        }
    };
    let llm_thinking_mode = match b
        .llm_thinking_mode
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        None | Some("server") => None,
        Some("on") => Some(WebClientLlmThinkingMode::On),
        Some("off") => Some(WebClientLlmThinkingMode::Off),
        Some(other) => {
            return Err(format!(
                "client_llm.llm_thinking_mode 非法: {other:?}（支持 server、on、off）"
            ));
        }
    };
    if api_base.is_none()
        && model.is_none()
        && api_key.is_none()
        && llm_context_tokens.is_none()
        && llm_thinking_mode.is_none()
    {
        return Ok(None);
    }
    validate_optional_web_api_base(&api_base, "client_llm")?;
    validate_optional_field_max_len(&model, CLIENT_LLM_MODEL_MAX, "client_llm", "model")?;
    validate_optional_field_max_len(&api_key, CLIENT_LLM_API_KEY_MAX, "client_llm", "api_key")?;
    Ok(Some(chat_job_queue::WebChatLlmOverride {
        api_base,
        model,
        api_key,
        llm_context_tokens,
        llm_thinking_mode,
    }))
}

pub(super) fn parse_executor_llm_override(
    raw: Option<ExecutorLlmBody>,
) -> Result<Option<chat_job_queue::WebChatLlmOverride>, String> {
    let Some(b) = raw else {
        return Ok(None);
    };
    let api_base = b
        .api_base
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let model = b
        .model
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let api_key = b
        .api_key
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    if api_base.is_none() && model.is_none() && api_key.is_none() {
        return Ok(None);
    }
    validate_optional_web_api_base(&api_base, "executor_llm")?;
    validate_optional_field_max_len(&model, CLIENT_LLM_MODEL_MAX, "executor_llm", "model")?;
    validate_optional_field_max_len(&api_key, CLIENT_LLM_API_KEY_MAX, "executor_llm", "api_key")?;
    Ok(Some(chat_job_queue::WebChatLlmOverride {
        api_base,
        model,
        api_key,
        llm_context_tokens: None,
        llm_thinking_mode: None,
    }))
}

pub(super) fn parse_execution_mode_override(
    raw: Option<String>,
) -> Result<Option<chat_job_queue::WebExecutionModeOverride>, String> {
    let Some(s) = raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    match s {
        "rolling_planning" => Ok(Some(
            chat_job_queue::WebExecutionModeOverride::RollingPlanning,
        )),
        "hierarchical" => Ok(Some(chat_job_queue::WebExecutionModeOverride::Hierarchical)),
        _ => Err(format!(
            "execution_mode 非法: {:?}（支持 rolling_planning、hierarchical）",
            s
        )),
    }
}

pub(super) fn parse_readonly_tool_ttl_cache_secs(raw: Option<u64>) -> Result<Option<u64>, String> {
    let Some(n) = raw else {
        return Ok(None);
    };
    if n > 3600 {
        return Err("readonly_tool_ttl_cache_secs 过大（上限 3600）".to_string());
    }
    Ok(Some(n))
}

fn effective_llm_api_key_for_web_chat(
    state: &AppState,
    ov: &Option<chat_job_queue::WebChatLlmOverride>,
) -> String {
    if let Some(o) = ov
        && let Some(ref k) = o.api_key
        && !k.trim().is_empty()
    {
        return k.clone();
    }
    state.http.api_key.clone()
}

pub(super) async fn ensure_bearer_api_key_for_chat(
    state: &AppState,
    llm_override: &Option<chat_job_queue::WebChatLlmOverride>,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    let auth = {
        let g = state.http.cfg.read().await;
        g.llm.llm_http_auth_mode
    };
    if auth != LlmHttpAuthMode::Bearer {
        return Ok(());
    }
    let k = effective_llm_api_key_for_web_chat(state, llm_override);
    if k.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "LLM_API_KEY_REQUIRED",
                message: "当前为 bearer 鉴权但未配置 LLM API 密钥：请在侧栏「设置」中填写「API 密钥」（仅存本机浏览器），或设置环境变量 API_KEY 后重启服务。"
                    .to_string(),
                reason_code: None,
            }),
        ));
    }
    Ok(())
}

pub(super) fn parse_optional_chat_temperature(raw: Option<f64>) -> Result<Option<f32>, String> {
    let Some(t) = raw else {
        return Ok(None);
    };
    if !t.is_finite() {
        return Err("temperature 须为有限浮点数".to_string());
    }
    let t = t as f32;
    if !(0.0..=2.0).contains(&t) {
        return Err("temperature 须在 0～2 之间".to_string());
    }
    Ok(Some(t))
}

pub(super) fn parse_seed_override_from_body(
    seed: Option<i64>,
    seed_policy: Option<String>,
) -> Result<crate::LlmSeedOverride, String> {
    let policy = seed_policy
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match (seed, policy) {
        (Some(_), Some(p)) if p.eq_ignore_ascii_case("omit") || p.eq_ignore_ascii_case("none") => {
            Err("seed 与 seed_policy=omit 不能同时使用".to_string())
        }
        (Some(n), _) => Ok(crate::LlmSeedOverride::Fixed(n)),
        (None, Some(p)) if p.eq_ignore_ascii_case("omit") || p.eq_ignore_ascii_case("none") => {
            Ok(crate::LlmSeedOverride::OmitFromRequest)
        }
        (None, Some(p)) => Err(format!(
            "未知的 seed_policy: {:?}（支持 omit、none 或省略）",
            p
        )),
        (None, None) => Ok(crate::LlmSeedOverride::FromConfig),
    }
}

pub(super) fn normalize_approval_session_id(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() || s.len() > 128 {
        return None;
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        return None;
    }
    Some(s.to_string())
}

/// 可选 `agent_role`：非空时与 `conversation_id` 同类字符约束，最长 64。
pub(crate) fn normalize_agent_role(raw: Option<&str>) -> Result<Option<String>, String> {
    const MAX: usize = 64;
    let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if s.len() > MAX {
        return Err(format!("agent_role 过长（最多 {MAX} 个字符）"));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        return Err("agent_role 仅允许字母、数字、- _ . :".to_string());
    }
    Ok(Some(s.to_string()))
}

pub(crate) fn normalize_client_conversation_id(
    raw: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(id) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if id.len() > CONVERSATION_ID_MAX_LEN {
        return Err(format!(
            "conversation_id 过长（最多 {} 个字符）",
            CONVERSATION_ID_MAX_LEN
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        return Err("conversation_id 仅允许字母、数字、- _ . :".to_string());
    }
    Ok(Some(id.to_string()))
}

#[cfg(test)]
mod web_llm_api_base_tests {
    use crate::web::http_types::chat::{ClientLlmBody, ExecutorLlmBody};

    use super::{parse_client_llm_override, parse_executor_llm_override};

    #[test]
    fn client_llm_rejects_metadata_ip_host() {
        let err = parse_client_llm_override(Some(ClientLlmBody {
            api_base: Some("http://169.254.169.254/latest/meta-data/".into()),
            ..Default::default()
        }))
        .unwrap_err();
        assert!(err.contains("主机"), "{err}");
    }

    #[test]
    fn executor_llm_accepts_localhost_gateways() {
        let ok = parse_executor_llm_override(Some(ExecutorLlmBody {
            api_base: Some("http://127.0.0.1:11434/v1".into()),
            ..Default::default()
        }));
        assert!(ok.is_ok(), "{:?}", ok.err());
    }
}
