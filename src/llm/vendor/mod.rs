//! 按 **OpenAI 兼容网关族** 调整出站 `chat/completions` 请求（温度、`thinking`、是否保留带 `tool_calls` 的 `reasoning_content` 等）。
//!
//! 新增厂商时：实现 [`LlmVendorAdapter`]，并在 [`llm_vendor_adapter_for_model`] / [`llm_vendor_adapter`] 中注册路由（优先 **`model` ID**；[`llm_vendor_adapter`] 还可参考 **`api_base`**）。

use crate::config::AgentConfig;

/// 对单次 `ChatRequest` 的厂商特有处理（构造阶段调用；HTTP 仍走 OpenAI 兼容 JSON）。
pub trait LlmVendorAdapter: Send + Sync {
    /// 出站 **`temperature`**（部分模型 ID 仅接受离散值）。
    fn coerce_temperature(&self, model: &str, temperature: f32) -> f32;

    /// 可选扩展字段 **`thinking`**（智谱、Moonshot Kimi 等）；`None` 则 JSON 省略。
    fn thinking_field(&self, cfg: &AgentConfig) -> Option<serde_json::Value>;

    /// 构造 `messages` 时：含 **`tool_calls`** 的 assistant 是否须保留 **`reasoning_content`**（如 Kimi k2.5 默认 thinking 时接口要求）。
    fn preserve_assistant_tool_call_reasoning(&self, cfg: &AgentConfig) -> bool;
}

// --------------------------------------------------------------------------- model id helpers (Kimi)

/// 是否为 Moonshot **kimi-k2.5** 系列模型 ID（**`kimi-k2.5`** 或 **`kimi-k2.5-…`**，大小写不敏感）。
#[inline]
pub(crate) fn is_kimi_k2_5_model(model: &str) -> bool {
    const PREFIX: &[u8] = b"kimi-k2.5";
    let b = model.as_bytes();
    b.len() >= PREFIX.len()
        && b[..PREFIX.len()].eq_ignore_ascii_case(PREFIX)
        && (b.len() == PREFIX.len() || b[PREFIX.len()] == b'-')
}

/// **`kimi-k2-thinking`** / **`kimi-k2-thinking-…`**（大小写不敏感）。
#[inline]
pub(crate) fn is_kimi_k2_thinking_model(model: &str) -> bool {
    const PREFIX: &[u8] = b"kimi-k2-thinking";
    let b = model.as_bytes();
    b.len() >= PREFIX.len()
        && b[..PREFIX.len()].eq_ignore_ascii_case(PREFIX)
        && (b.len() == PREFIX.len() || b[PREFIX.len()] == b'-')
}

/// **`kimi-k2`** 或 **`kimi-k2-…`**，但**不是** **`kimi-k2.5`** / **`kimi-k2-thinking`** 分支。
#[inline]
fn is_kimi_k2_fixed_temperature_zero_six_family(model: &str) -> bool {
    const PREFIX: &[u8] = b"kimi-k2";
    let b = model.as_bytes();
    if !(b.len() >= PREFIX.len() && b[..PREFIX.len()].eq_ignore_ascii_case(PREFIX)) {
        return false;
    }
    if b.len() == PREFIX.len() {
        return true;
    }
    if b[PREFIX.len()] != b'-' {
        return false;
    }
    !is_kimi_k2_5_model(model) && !is_kimi_k2_thinking_model(model)
}

/// 是否走 **Moonshot Kimi** 采样与消息规则（当前为 **`kimi-k2` 前缀** 族，含 `kimi-k2.5`、`kimi-k2-thinking`、`kimi-k2-0905` 等）。
#[inline]
pub(crate) fn is_moonshot_kimi_family_model_id(model: &str) -> bool {
    let b = model.as_bytes();
    const P: &[u8] = b"kimi-k2";
    b.len() >= P.len() && b[..P.len()].eq_ignore_ascii_case(P)
}

// --------------------------------------------------------------------------- model id helpers (智谱 GLM / MiniMax)

/// **`glm-` 前缀**（如 **`glm-5`**、**`glm-4.7`**），大小写不敏感。
#[inline]
pub(crate) fn is_zhipu_glm_family_model_id(model: &str) -> bool {
    let b = model.as_bytes();
    const P: &[u8] = b"glm-";
    b.len() >= P.len() && b[..P.len()].eq_ignore_ascii_case(P)
}

/// **MiniMax** 常见 OpenAI 兼容 **`model`**：`MiniMax-…`（大小写不敏感）；兼容部分 **`abab`** 前缀旧 ID。
#[inline]
pub(crate) fn is_minimax_family_model_id(model: &str) -> bool {
    let b = model.as_bytes();
    const M: &[u8] = b"minimax-";
    if b.len() >= M.len() && b[..M.len()].eq_ignore_ascii_case(M) {
        return true;
    }
    const A: &[u8] = b"abab";
    b.len() >= A.len() && b[..A.len()].eq_ignore_ascii_case(A)
}

#[inline]
fn api_base_looks_zhipu_bigmodel(base: &str) -> bool {
    base.to_ascii_lowercase().contains("bigmodel.cn")
}

#[inline]
fn api_base_looks_minimax(base: &str) -> bool {
    let b = base.to_ascii_lowercase();
    b.contains("minimax")
}

/// TOML/环境变量均未设置 **`llm_reasoning_split`** 时的默认值：**MiniMax** 网关为 **`true`**，否则 **`false`**。
#[inline]
pub(crate) fn default_llm_reasoning_split_for_gateway(model: &str, api_base: &str) -> bool {
    is_minimax_family_model_id(model) || api_base_looks_minimax(api_base)
}

/// 出站是否将独立 **`system`** 折叠进 **`user`**：**MiniMax**（按 `model` / `api_base` 与 [`llm_vendor_adapter`] 同源规则识别）为 **`true`**，其余为 **`false`**（不再由 TOML / 环境变量配置）。
#[inline]
pub fn fold_system_into_user_for_config(cfg: &AgentConfig) -> bool {
    is_minimax_family_model_id(&cfg.model) || api_base_looks_minimax(&cfg.api_base)
}

fn kimi_coerce_temperature(model: &str, temperature: f32) -> f32 {
    if is_kimi_k2_thinking_model(model) {
        return 1.0;
    }
    if is_kimi_k2_5_model(model) {
        return 1.0;
    }
    if is_kimi_k2_fixed_temperature_zero_six_family(model) {
        return 0.6;
    }
    temperature
}

// --------------------------------------------------------------------------- adapters

/// 默认：温度原样；**不**写智谱专用 **`thinking`**（见 [`ZhipuGlmVendor`]）；不强制保留 tool 轮 `reasoning_content`。
#[derive(Debug, Copy, Clone, Default)]
pub struct GenericOpenAiCompatVendor;

impl LlmVendorAdapter for GenericOpenAiCompatVendor {
    fn coerce_temperature(&self, _model: &str, temperature: f32) -> f32 {
        temperature
    }

    fn thinking_field(&self, _cfg: &AgentConfig) -> Option<serde_json::Value> {
        None
    }

    fn preserve_assistant_tool_call_reasoning(&self, _cfg: &AgentConfig) -> bool {
        false
    }
}

/// 智谱 **GLM**（OpenAI 兼容）：**`llm_bigmodel_thinking`** 时写入 **`thinking: { "type": "enabled" }`**（见 GLM-5 文档）。
#[derive(Debug, Copy, Clone, Default)]
pub struct ZhipuGlmVendor;

impl LlmVendorAdapter for ZhipuGlmVendor {
    fn coerce_temperature(&self, _model: &str, temperature: f32) -> f32 {
        temperature
    }

    fn thinking_field(&self, cfg: &AgentConfig) -> Option<serde_json::Value> {
        if cfg.llm_bigmodel_thinking {
            return Some(serde_json::json!({ "type": "enabled" }));
        }
        None
    }

    fn preserve_assistant_tool_call_reasoning(&self, _cfg: &AgentConfig) -> bool {
        false
    }
}

/// **MiniMax**：不在此写智谱 **`thinking`**；**`reasoning_split`** 仍由 [`crate::types::ChatRequest`] / `tool_chat_request` 按配置组装。
#[derive(Debug, Copy, Clone, Default)]
pub struct MiniMaxVendor;

impl LlmVendorAdapter for MiniMaxVendor {
    fn coerce_temperature(&self, _model: &str, temperature: f32) -> f32 {
        temperature
    }

    fn thinking_field(&self, _cfg: &AgentConfig) -> Option<serde_json::Value> {
        None
    }

    fn preserve_assistant_tool_call_reasoning(&self, _cfg: &AgentConfig) -> bool {
        false
    }
}

/// Moonshot Kimi（`kimi-k2*`）：温度钳制、**k2.5** 可写 **`thinking: disabled`**、k2.5+默认 thinking 时保留 tool 轮 `reasoning_content`。
#[derive(Debug, Copy, Clone, Default)]
pub struct MoonshotKimiVendor;

impl LlmVendorAdapter for MoonshotKimiVendor {
    fn coerce_temperature(&self, model: &str, temperature: f32) -> f32 {
        kimi_coerce_temperature(model, temperature)
    }

    fn thinking_field(&self, cfg: &AgentConfig) -> Option<serde_json::Value> {
        if cfg.llm_kimi_thinking_disabled && is_kimi_k2_5_model(&cfg.model) {
            return Some(serde_json::json!({ "type": "disabled" }));
        }
        if cfg.llm_bigmodel_thinking {
            return Some(serde_json::json!({ "type": "enabled" }));
        }
        None
    }

    fn preserve_assistant_tool_call_reasoning(&self, cfg: &AgentConfig) -> bool {
        is_kimi_k2_5_model(&cfg.model) && !cfg.llm_kimi_thinking_disabled
    }
}

static GENERIC: GenericOpenAiCompatVendor = GenericOpenAiCompatVendor;
static KIMI: MoonshotKimiVendor = MoonshotKimiVendor;
static GLM: ZhipuGlmVendor = ZhipuGlmVendor;
static MINIMAX: MiniMaxVendor = MiniMaxVendor;

/// 按 **`model`** 与 **`api_base`** 选择适配器（**`model` 族优先**，避免误用 `api_base` 覆盖 Kimi 等明确 ID）。
#[inline]
pub fn llm_vendor_adapter(cfg: &AgentConfig) -> &'static dyn LlmVendorAdapter {
    if is_moonshot_kimi_family_model_id(&cfg.model) {
        return &KIMI;
    }
    if is_minimax_family_model_id(&cfg.model) || api_base_looks_minimax(&cfg.api_base) {
        return &MINIMAX;
    }
    if is_zhipu_glm_family_model_id(&cfg.model) || api_base_looks_zhipu_bigmodel(&cfg.api_base) {
        return &GLM;
    }
    &GENERIC
}

/// 仅按 **`model`** ID 选择适配器（无 `AgentConfig` 时用于温度等逻辑；**不含** `api_base` 回退）。
#[inline]
#[allow(dead_code)] // 对外 API + `vendor_temperature_for_model`；默认 lib 构建可能无其它调用
pub fn llm_vendor_adapter_for_model(model: &str) -> &'static dyn LlmVendorAdapter {
    if is_moonshot_kimi_family_model_id(model) {
        &KIMI
    } else if is_minimax_family_model_id(model) {
        &MINIMAX
    } else if is_zhipu_glm_family_model_id(model) {
        &GLM
    } else {
        &GENERIC
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load_config;

    /// 避免单测受工作区 `config.toml` 中 `api_base`（若含 `minimax` 会优先匹配 MiniMax 适配器）影响。
    fn cfg_neutral_deepseek_base() -> crate::config::AgentConfig {
        let mut cfg = load_config(None).expect("default embedded config");
        cfg.api_base = "https://api.deepseek.com/v1".to_string();
        cfg
    }

    #[test]
    fn bigmodel_thinking_inserts_json_when_enabled_on_glm() {
        let mut cfg = cfg_neutral_deepseek_base();
        cfg.model = "glm-5".to_string();
        cfg.llm_bigmodel_thinking = false;
        let v = llm_vendor_adapter(&cfg);
        assert!(v.thinking_field(&cfg).is_none());

        cfg.llm_bigmodel_thinking = true;
        let v = llm_vendor_adapter(&cfg);
        let t = v.thinking_field(&cfg).expect("thinking");
        assert_eq!(t.get("type").and_then(|x| x.as_str()), Some("enabled"));
    }

    #[test]
    fn bigmodel_thinking_not_sent_for_generic_deepseek() {
        let mut cfg = cfg_neutral_deepseek_base();
        cfg.model = "deepseek-chat".to_string();
        cfg.llm_bigmodel_thinking = true;
        let v = llm_vendor_adapter(&cfg);
        assert!(
            v.thinking_field(&cfg).is_none(),
            "智谱 thinking 仅应由 ZhipuGlmVendor 写出"
        );
    }

    #[test]
    fn bigmodel_thinking_not_sent_for_minimax() {
        let mut cfg = cfg_neutral_deepseek_base();
        cfg.model = "MiniMax-M2.7".to_string();
        cfg.llm_bigmodel_thinking = true;
        let v = llm_vendor_adapter(&cfg);
        assert!(v.thinking_field(&cfg).is_none());
    }

    #[test]
    fn api_base_bigmodel_cn_routes_to_glm_vendor() {
        let mut cfg = cfg_neutral_deepseek_base();
        cfg.api_base = "https://open.bigmodel.cn/api/paas/v4".to_string();
        cfg.model = "some-custom-route-id".to_string();
        cfg.llm_bigmodel_thinking = true;
        let v = llm_vendor_adapter(&cfg);
        assert!(v.thinking_field(&cfg).is_some());
    }

    #[test]
    fn api_base_minimax_routes_to_minimax_vendor() {
        let mut cfg = cfg_neutral_deepseek_base();
        cfg.api_base = "https://api.minimaxi.com/v1".to_string();
        cfg.model = "deepseek-chat".to_string();
        cfg.llm_bigmodel_thinking = true;
        let v = llm_vendor_adapter(&cfg);
        assert!(v.thinking_field(&cfg).is_none());
    }

    #[test]
    fn kimi_thinking_disabled_inserts_json_when_enabled() {
        let mut cfg = cfg_neutral_deepseek_base();
        cfg.model = "kimi-k2.5".to_string();
        cfg.llm_bigmodel_thinking = false;
        cfg.llm_kimi_thinking_disabled = true;
        let v = llm_vendor_adapter(&cfg);
        let t = v.thinking_field(&cfg).expect("thinking");
        assert_eq!(t.get("type").and_then(|x| x.as_str()), Some("disabled"));
    }

    #[test]
    fn kimi_thinking_disabled_wins_over_bigmodel_thinking() {
        let mut cfg = cfg_neutral_deepseek_base();
        cfg.model = "kimi-k2.5".to_string();
        cfg.llm_bigmodel_thinking = true;
        cfg.llm_kimi_thinking_disabled = true;
        let v = llm_vendor_adapter(&cfg);
        let t = v.thinking_field(&cfg).expect("thinking");
        assert_eq!(t.get("type").and_then(|x| x.as_str()), Some("disabled"));
    }

    #[test]
    fn kimi_k2_5_temperature_coerced_to_one() {
        let k = MoonshotKimiVendor;
        assert_eq!(k.coerce_temperature("kimi-k2.5", 0.3), 1.0);
        assert_eq!(k.coerce_temperature("kimi-k2.5-preview", 0.3), 1.0);
        assert_eq!(k.coerce_temperature("Kimi-K2.5", 0.0), 1.0);
        let g = GenericOpenAiCompatVendor;
        assert_eq!(g.coerce_temperature("deepseek-chat", 0.3), 0.3);
        let z = ZhipuGlmVendor;
        assert_eq!(z.coerce_temperature("glm-5", 0.3), 0.3);
        assert_eq!(k.coerce_temperature("kimi-k2-thinking", 0.3), 1.0);
        assert_eq!(k.coerce_temperature("kimi-k2-thinking-turbo", 0.2), 1.0);
        assert_eq!(k.coerce_temperature("kimi-k2-0905-preview", 0.3), 0.6);
        assert_eq!(k.coerce_temperature("kimi-k2", 0.9), 0.6);
        assert_eq!(k.coerce_temperature("kimi-k2.51-hypothetical", 0.3), 0.3);
    }

    #[test]
    fn default_llm_reasoning_split_true_for_minimax_model() {
        assert!(super::default_llm_reasoning_split_for_gateway(
            "MiniMax-M2.7",
            "https://api.deepseek.com/v1"
        ));
    }

    #[test]
    fn default_llm_reasoning_split_true_for_minimax_api() {
        assert!(super::default_llm_reasoning_split_for_gateway(
            "some-id",
            "https://api.minimaxi.com/v1"
        ));
    }

    #[test]
    fn default_llm_reasoning_split_false_for_generic() {
        assert!(!super::default_llm_reasoning_split_for_gateway(
            "deepseek-chat",
            "https://api.deepseek.com/v1"
        ));
    }

    #[test]
    fn fold_system_into_user_true_for_minimax_model() {
        let mut cfg = cfg_neutral_deepseek_base();
        cfg.model = "MiniMax-M2.7".to_string();
        assert!(super::fold_system_into_user_for_config(&cfg));
    }

    #[test]
    fn fold_system_into_user_true_for_minimax_api_base() {
        let mut cfg = cfg_neutral_deepseek_base();
        cfg.api_base = "https://api.minimaxi.com/v1".to_string();
        assert!(super::fold_system_into_user_for_config(&cfg));
    }

    #[test]
    fn fold_system_into_user_false_for_deepseek_default() {
        let cfg = cfg_neutral_deepseek_base();
        assert!(!super::fold_system_into_user_for_config(&cfg));
    }
}
