//! 请求体验证与规范化（conversation_id、client_llm、seed、temperature 等）。

use axum::Json;
use axum::http::StatusCode;

use super::super::app_state::{AppState, CONVERSATION_ID_MAX_LEN};
use crate::chat_job_queue;
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
    if api_base.is_none() && model.is_none() && api_key.is_none() {
        return Ok(None);
    }
    if let Some(ref s) = api_base
        && s.len() > CLIENT_LLM_API_BASE_MAX
    {
        return Err(format!(
            "client_llm.api_base 过长（上限 {} 字符）",
            CLIENT_LLM_API_BASE_MAX
        ));
    }
    if let Some(ref s) = model
        && s.len() > CLIENT_LLM_MODEL_MAX
    {
        return Err(format!(
            "client_llm.model 过长（上限 {} 字符）",
            CLIENT_LLM_MODEL_MAX
        ));
    }
    if let Some(ref s) = api_key
        && s.len() > CLIENT_LLM_API_KEY_MAX
    {
        return Err(format!(
            "client_llm.api_key 过长（上限 {} 字符）",
            CLIENT_LLM_API_KEY_MAX
        ));
    }
    Ok(Some(chat_job_queue::WebChatLlmOverride {
        api_base,
        model,
        api_key,
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
    if let Some(ref s) = api_base
        && s.len() > CLIENT_LLM_API_BASE_MAX
    {
        return Err(format!(
            "executor_llm.api_base 过长（上限 {} 字符）",
            CLIENT_LLM_API_BASE_MAX
        ));
    }
    if let Some(ref s) = model
        && s.len() > CLIENT_LLM_MODEL_MAX
    {
        return Err(format!(
            "executor_llm.model 过长（上限 {} 字符）",
            CLIENT_LLM_MODEL_MAX
        ));
    }
    if let Some(ref s) = api_key
        && s.len() > CLIENT_LLM_API_KEY_MAX
    {
        return Err(format!(
            "executor_llm.api_key 过长（上限 {} 字符）",
            CLIENT_LLM_API_KEY_MAX
        ));
    }
    Ok(Some(chat_job_queue::WebChatLlmOverride {
        api_base,
        model,
        api_key,
    }))
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
    state.api_key.clone()
}

pub(super) async fn ensure_bearer_api_key_for_chat(
    state: &AppState,
    llm_override: &Option<chat_job_queue::WebChatLlmOverride>,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    let auth = {
        let g = state.cfg.read().await;
        g.llm_http_auth_mode
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
