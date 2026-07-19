//! LLM 客户端配置类型（供 `crabmate-llm` 与 `crabmate-config` 共用，避免配置加载层膨胀 LLM 编译图）。

// ---------------------------------------------------------------------------
// LlmHttpAuthMode
// ---------------------------------------------------------------------------

/// 对 OpenAI 兼容 **`POST …/chat/completions`**（及同基址 **`GET …/models`**）的 HTTP 鉴权方式。
///
/// 本地 **Ollama** 等默认无需密钥时可设为 [`Self::None`]，进程可不设 **`API_KEY`** 且不发送 `Authorization`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LlmHttpAuthMode {
    /// `Authorization: Bearer {API_KEY}`（云端 OpenAI 兼容服务默认）。
    #[default]
    Bearer,
    /// 不附加 `Authorization`；**`API_KEY` 环境变量可省略**。
    None,
}

impl LlmHttpAuthMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "bearer" => Ok(Self::Bearer),
            "none" | "off" | "false" | "no" | "no_auth" => Ok(Self::None),
            _ => Err(format!(
                "未知的 llm_http_auth_mode: {:?}（支持 bearer、none）",
                s.trim()
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bearer => "bearer",
            Self::None => "none",
        }
    }
}

// ---------------------------------------------------------------------------
// LlmConnectionConfig
// ---------------------------------------------------------------------------

/// LLM 网关连接与认证。
#[derive(Debug, Clone)]
pub struct LlmConnectionConfig {
    pub api_base: String,
    pub model: String,
    pub planner_model: Option<String>,
    pub executor_model: Option<String>,
    pub llm_http_auth_mode: LlmHttpAuthMode,
}

// ---------------------------------------------------------------------------
// LlmSamplingConfig
// ---------------------------------------------------------------------------

/// 采样与上下文窗口计量。
#[derive(Debug, Clone)]
pub struct LlmSamplingConfig {
    pub max_tokens: u32,
    pub llm_context_tokens: u32,
    pub temperature: f32,
    pub llm_seed: Option<i64>,
}

// ---------------------------------------------------------------------------
// LlmVendorFlagsConfig
// ---------------------------------------------------------------------------

/// 供应商专属请求开关。
#[derive(Debug, Clone)]
pub struct LlmVendorFlagsConfig {
    pub llm_reasoning_split: bool,
    pub llm_bigmodel_thinking: bool,
    pub llm_kimi_thinking_disabled: bool,
}

// ---------------------------------------------------------------------------
// LlmHttpRetryConfig
// ---------------------------------------------------------------------------

/// `chat/completions` HTTP 客户端退避。
#[derive(Debug, Clone)]
pub struct LlmHttpRetryConfig {
    pub api_timeout_secs: u64,
    pub api_max_retries: u32,
    pub api_retry_delay_secs: u64,
}

// ---------------------------------------------------------------------------
// LlmConfig — LLM 层运行时视图（crabmate-llm 的 cfg 参数类型）
// ---------------------------------------------------------------------------

/// `crabmate-llm` 需要的全部配置字段聚合（替代 `crabmate_config::AgentConfig` 子集）。
///
/// 由上层（根包 `crabmate::llm`）从完整 `AgentConfig` 构造后传入。
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub llm: LlmConnectionConfig,
    pub sampling: LlmSamplingConfig,
    pub vendor_flags: LlmVendorFlagsConfig,
    pub http_retry: LlmHttpRetryConfig,
}

// ---------------------------------------------------------------------------
// Gateway hints 函数
// ---------------------------------------------------------------------------

/// **MiniMax** 常见 OpenAI 兼容 **`model`**：`MiniMax-…`（大小写不敏感）；兼容部分 **`abab`** 前缀旧 ID。
#[inline]
fn is_minimax_family_model_id(model: &str) -> bool {
    let b = model.as_bytes();
    const M: &[u8] = b"minimax-";
    if b.len() >= M.len() && b[..M.len()].eq_ignore_ascii_case(M) {
        return true;
    }
    const A: &[u8] = b"abab";
    b.len() >= A.len() && b[..A.len()].eq_ignore_ascii_case(A)
}

#[inline]
fn api_base_looks_minimax(base: &str) -> bool {
    base.to_ascii_lowercase().contains("minimax")
}

/// TOML/环境变量均未设置 **`llm_reasoning_split`** 时的默认值：**MiniMax** 网关为 **`true`**，否则 **`false`**。
#[inline]
pub fn default_llm_reasoning_split_for_gateway(model: &str, api_base: &str) -> bool {
    is_minimax_family_model_id(model) || api_base_looks_minimax(api_base)
}

/// 出站是否将独立 **`system`** 折叠进 **`user`**：**MiniMax** 为 **`true`**，其余为 **`false`**。
#[inline]
pub fn fold_system_into_user_for_gateway(model: &str, api_base: &str) -> bool {
    is_minimax_family_model_id(model) || api_base_looks_minimax(api_base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_http_auth_mode_parse_bearer_and_none() {
        assert_eq!(
            LlmHttpAuthMode::parse("bearer").unwrap(),
            LlmHttpAuthMode::Bearer
        );
        assert_eq!(
            LlmHttpAuthMode::parse("NONE").unwrap(),
            LlmHttpAuthMode::None
        );
        assert_eq!(
            LlmHttpAuthMode::parse("no_auth").unwrap(),
            LlmHttpAuthMode::None
        );
    }

    #[test]
    fn default_reasoning_split_true_for_minimax() {
        assert!(default_llm_reasoning_split_for_gateway(
            "MiniMax-M2.7",
            "https://api.deepseek.com/v1"
        ));
        assert!(default_llm_reasoning_split_for_gateway(
            "some-id",
            "https://api.minimaxi.com/v1"
        ));
    }

    #[test]
    fn default_reasoning_split_false_for_generic() {
        assert!(!default_llm_reasoning_split_for_gateway(
            "deepseek-chat",
            "https://api.deepseek.com/v1"
        ));
    }

    #[test]
    fn fold_system_into_user_true_for_minimax() {
        assert!(fold_system_into_user_for_gateway(
            "MiniMax-M2.7",
            "https://api.deepseek.com/v1"
        ));
        assert!(fold_system_into_user_for_gateway(
            "some-id",
            "https://api.minimaxi.com/v1"
        ));
    }

    #[test]
    fn fold_system_into_user_false_for_deepseek() {
        assert!(!fold_system_into_user_for_gateway(
            "deepseek-chat",
            "https://api.deepseek.com/v1"
        ));
    }
}
