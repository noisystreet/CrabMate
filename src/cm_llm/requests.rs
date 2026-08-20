//! `ChatRequest` 惯用构造（工具轮 / 无工具轮等）。

use crate::cm_llm::vendor::{
    api_base_looks_volcano_engine_openai_compat, deepseek_json_output_eligible,
    deepseek_reasoning_effort_for_request, flatten_image_url_parts_for_config,
    fold_system_into_user_for_config, llm_vendor_adapter,
};
use crate::cm_llm::vendor_messages::{
    conversation_messages_to_vendor_body, normalize_stripped_messages_for_vendor_body,
};
use crate::cm_types::llm_config::LlmConfig;
use crate::cm_types::{
    ChatRequest, ChatRequestCore, ChatRequestVendorExtensions, LlmSeedOverride, Message, Tool,
    is_long_term_memory_injection, messages_for_api_stripping_reasoning_skip_ui_separators,
    resolved_llm_seed,
};

/// **kimi-k2.5** 在**未**显式关闭思考时，服务端 **`thinking` 默认启用**；此时含 **`tool_calls`** 的 assistant 历史消息必须带 **`reasoning_content`**，否则返回 `invalid_request_error`（见 Moonshot [Chat API](https://platform.moonshot.cn/docs/api/chat) 与实测报错）。
#[inline]
pub fn kimi_k2_5_vendor_requires_tool_call_reasoning(cfg: &LlmConfig) -> bool {
    llm_vendor_adapter(&cfg.llm.model, &cfg.llm.api_base)
        .preserve_assistant_tool_call_reasoning(cfg)
}

/// 按 **`AgentConfig`**（**`model` + `api_base`**）钳制温度（摘要等路径与 [`llm_vendor_adapter`] 一致）。
#[inline]
pub fn vendor_temperature_for_config(cfg: &LlmConfig, temperature: f32) -> f32 {
    let effective_model = &cfg.llm.model;
    llm_vendor_adapter(&cfg.llm.model, &cfg.llm.api_base)
        .coerce_temperature(effective_model, temperature)
}

/// Agent 主路径与普通 LLM 调用共用的 **`ChatRequestVendorExtensions`**（工具轮 / 无工具轮）。
#[inline]
pub fn chat_request_vendor_extensions_for_agent(cfg: &LlmConfig) -> ChatRequestVendorExtensions {
    let v = llm_vendor_adapter(&cfg.llm.model, &cfg.llm.api_base);
    // 方舟等网关会将 MiniMax 扩展键 **`reasoning_split`** 视为非法参数（HTTP 400），即便配置或环境变量曾打开。
    let reasoning_split = if api_base_looks_volcano_engine_openai_compat(&cfg.llm.api_base) {
        None
    } else {
        cfg.vendor_flags.llm_reasoning_split.then_some(true)
    };
    ChatRequestVendorExtensions {
        reasoning_split,
        thinking: v.thinking_field(cfg),
        reasoning_effort: deepseek_reasoning_effort_for_request(
            &cfg.llm.api_base,
            &cfg.vendor_flags,
        ),
        response_format: None,
    }
}

/// 构造带 tools、**`tool_choice: auto`** 及采样参数的请求体（`stream` 由 HTTP 层按 `no_stream` 覆盖）。
pub fn tool_chat_request(
    cfg: &LlmConfig,
    messages: &[Message],
    tools: &[Tool],
    temperature_override: Option<f32>,
    model_override: Option<&str>,
    seed_override: LlmSeedOverride,
) -> ChatRequest {
    let v = llm_vendor_adapter(&cfg.llm.model, &cfg.llm.api_base);
    let effective_model = model_override.unwrap_or(&cfg.llm.model);
    ChatRequest {
        core: ChatRequestCore {
            model: effective_model.to_string(),
            messages: conversation_messages_to_vendor_body(
                messages,
                fold_system_into_user_for_config(effective_model, &cfg.llm.api_base),
                v.preserve_assistant_tool_call_reasoning(cfg),
                deepseek_json_output_eligible(&cfg.llm.api_base),
                flatten_image_url_parts_for_config(effective_model, &cfg.llm.api_base),
            ),
            tools: Some(tools.to_vec()),
            tool_choice: Some("auto".to_string()),
            max_tokens: cfg.sampling.max_tokens,
            temperature: v.coerce_temperature(
                effective_model,
                temperature_override.unwrap_or(cfg.sampling.temperature),
            ),
            seed: resolved_llm_seed(cfg.sampling.llm_seed, seed_override),
            stream: None,
        },
        vendor: chat_request_vendor_extensions_for_agent(cfg),
    }
}

/// 构造**显式禁止工具调用**的请求（`tools: []` + `tool_choice: "none"`），用于分阶段规划轮等。
/// 按 OpenAI API 语义硬性禁止模型返回 `tool_calls`，比省略 `tools` 字段（`None`）更可靠。
/// 对 `messages` 先做 [`messages_for_api_stripping_reasoning_skip_ui_separators`] 再 normalize；进程内分阶段路径优先 [`no_tools_chat_request_from_messages`] 以避免二次 strip。
#[allow(dead_code)] // 公共 API；单测覆盖等价性，主进程分阶段路径用 `no_tools_chat_request_from_messages`
pub fn no_tools_chat_request(
    cfg: &LlmConfig,
    messages: &[Message],
    temperature_override: Option<f32>,
    model_override: Option<&str>,
    seed_override: LlmSeedOverride,
) -> ChatRequest {
    no_tools_chat_request_from_messages(
        cfg,
        messages_for_api_stripping_reasoning_skip_ui_separators(
            messages,
            kimi_k2_5_vendor_requires_tool_call_reasoning(cfg),
            deepseek_json_output_eligible(&cfg.llm.api_base),
        ),
        temperature_override,
        model_override,
        seed_override,
    )
}

/// 与 [`no_tools_chat_request`] 相同，但接受**已**按规划轮规则拼好的 `messages`（通常已不含 UI 分隔线且已剥离 `reasoning_content`），再剔除 [`is_long_term_memory_injection`]，仅经 normalize，避免对同一会话再做一轮全量 `strip`。
pub fn no_tools_chat_request_from_messages(
    cfg: &LlmConfig,
    messages: Vec<Message>,
    temperature_override: Option<f32>,
    model_override: Option<&str>,
    seed_override: LlmSeedOverride,
) -> ChatRequest {
    let messages: Vec<Message> = messages
        .into_iter()
        .filter(|m| !is_long_term_memory_injection(m))
        .collect();
    let v = llm_vendor_adapter(&cfg.llm.model, &cfg.llm.api_base);
    let effective_model = model_override.unwrap_or(&cfg.llm.model);
    ChatRequest {
        core: ChatRequestCore {
            model: effective_model.to_string(),
            messages: normalize_stripped_messages_for_vendor_body(
                messages,
                fold_system_into_user_for_config(effective_model, &cfg.llm.api_base),
                flatten_image_url_parts_for_config(effective_model, &cfg.llm.api_base),
            ),
            tools: Some(vec![]),
            tool_choice: Some("none".to_string()),
            max_tokens: cfg.sampling.max_tokens,
            temperature: v.coerce_temperature(
                effective_model,
                temperature_override.unwrap_or(cfg.sampling.temperature),
            ),
            seed: resolved_llm_seed(cfg.sampling.llm_seed, seed_override),
            stream: None,
        },
        vendor: chat_request_vendor_extensions_for_agent(cfg),
    }
}

#[cfg(test)]
mod tests {
    use crate::cm_config::load_config;
    use crate::cm_types::llm_config::LlmConfig;
    use crate::cm_types::{
        LlmSeedOverride, Message, OPENAI_CHAT_COMPLETIONS_REL_PATH, OPENAI_MODELS_REL_PATH,
        messages_for_api_stripping_reasoning_skip_ui_separators,
    };

    fn agent_to_llm_cfg(cfg: &crate::cm_config::AgentConfig) -> LlmConfig {
        LlmConfig {
            llm: cfg.llm.clone(),
            sampling: cfg.llm_sampling.clone(),
            vendor_flags: cfg.llm_vendor_flags.clone(),
            http_retry: cfg.llm_http_retry.clone(),
        }
    }

    #[test]
    fn completions_path_matches_openai_compat() {
        assert_eq!(OPENAI_CHAT_COMPLETIONS_REL_PATH, "chat/completions");
    }

    #[test]
    fn models_path_matches_openai_compat() {
        assert_eq!(OPENAI_MODELS_REL_PATH, "models");
    }

    #[test]
    fn no_tools_chat_request_matches_from_messages_after_strip_skip_sep() {
        let cfg = load_config(None).expect("default embedded config");
        let llm_cfg = agent_to_llm_cfg(&cfg);
        let sep = Message::chat_ui_separator(true);
        let assistant = Message {
            role: "assistant".to_string(),
            content: Some("c".into()),
            reasoning_content: Some("r".to_string()),
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let messages = vec![Message::user_only("u"), sep, assistant];
        let a = super::no_tools_chat_request(
            &llm_cfg,
            &messages,
            None,
            None,
            LlmSeedOverride::FromConfig,
        );
        let stripped =
            messages_for_api_stripping_reasoning_skip_ui_separators(&messages, false, false);
        let b = super::no_tools_chat_request_from_messages(
            &llm_cfg,
            stripped,
            None,
            None,
            LlmSeedOverride::FromConfig,
        );
        assert_eq!(a.messages, b.messages);
        assert_eq!(a.tool_choice, b.tool_choice);
        assert_eq!(a.tools.as_ref().map(|t| t.len()), Some(0));
    }

    #[test]
    fn tool_chat_request_coerces_temperature_for_kimi_k2_5_model() {
        let mut cfg = load_config(None).expect("default embedded config");
        cfg.llm.model = "kimi-k2.5".to_string();
        cfg.llm_sampling.temperature = 0.3;
        let llm_cfg = agent_to_llm_cfg(&cfg);
        let req = super::tool_chat_request(
            &llm_cfg,
            &[Message::user_only("hi")],
            &[],
            None,
            None,
            LlmSeedOverride::FromConfig,
        );
        assert_eq!(req.temperature, 1.0);
        let req = super::tool_chat_request(
            &llm_cfg,
            &[Message::user_only("hi")],
            &[],
            Some(0.7),
            None,
            LlmSeedOverride::FromConfig,
        );
        assert_eq!(req.temperature, 1.0);
    }

    #[test]
    fn volcano_api_base_omits_reasoning_split_even_when_flag_true() {
        let mut cfg = load_config(None).expect("default embedded config");
        cfg.llm.api_base = "https://ark.cn-beijing.volces.com/api/coding/v3".to_string();
        cfg.llm.model = "Kimi-K2.6".to_string();
        cfg.llm_vendor_flags.llm_reasoning_split = true;
        let llm_cfg = agent_to_llm_cfg(&cfg);
        let ext = super::chat_request_vendor_extensions_for_agent(&llm_cfg);
        assert!(ext.reasoning_split.is_none());
    }

    #[test]
    fn tool_chat_request_flattens_image_url_for_deepseek() {
        let mut cfg = load_config(None).expect("default embedded config");
        cfg.llm.api_base = "https://api.deepseek.com/v1".to_string();
        cfg.llm.model = "deepseek-chat".to_string();
        let llm_cfg = agent_to_llm_cfg(&cfg);
        let msgs = [crate::cm_types::message_user_with_images(
            "看图",
            &["/uploads/shot.png".to_string()],
        )];
        let req = super::tool_chat_request(
            &llm_cfg,
            &msgs,
            &[],
            None,
            None,
            LlmSeedOverride::FromConfig,
        );
        match &req.messages.last().expect("user").content {
            Some(crate::cm_types::MessageContent::Text(s)) => {
                assert!(s.contains("看图"));
                assert!(s.contains("/uploads/shot.png"));
                assert!(s.contains("无法查看像素"));
            }
            other => panic!("expected flattened Text, got {other:?}"),
        }
    }

    #[test]
    fn tool_chat_request_flatten_follows_effective_model_not_cfg_model() {
        let mut cfg = load_config(None).expect("default embedded config");
        cfg.llm.api_base = "https://api.openai.com/v1".to_string();
        cfg.llm.model = "MiniMax-M2.7".to_string();
        let llm_cfg = agent_to_llm_cfg(&cfg);
        let msgs = [crate::cm_types::message_user_with_images(
            "看图",
            &["/uploads/shot.png".to_string()],
        )];
        let req = super::tool_chat_request(
            &llm_cfg,
            &msgs,
            &[],
            None,
            Some("gpt-4o"),
            LlmSeedOverride::FromConfig,
        );
        match &req.messages.last().expect("user").content {
            Some(crate::cm_types::MessageContent::Parts(_)) => {}
            other => panic!("override gpt-4o should keep image_url parts, got {other:?}"),
        }
    }
}
