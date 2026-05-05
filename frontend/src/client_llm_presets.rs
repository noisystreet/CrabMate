//! Web「设置」中 `client_llm.api_base` 的常用 OpenAI 兼容网关预设（与文档/README 中供应商示例一致）。

/// 单条预设：`id` 用于 i18n 与 `<select>` 的 value；`url` 写入 `api_base`（空串表示不覆盖服务端）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientLlmApiBasePreset {
    pub id: &'static str,
    pub url: &'static str,
    /// 选此项时，若模型名为空则填入建议值（不覆盖用户已填的 model）。
    pub suggested_model: Option<&'static str>,
}

pub const CLIENT_LLM_API_BASE_PRESETS: &[ClientLlmApiBasePreset] = &[
    ClientLlmApiBasePreset {
        id: "server",
        url: "",
        suggested_model: None,
    },
    ClientLlmApiBasePreset {
        id: "ollama",
        url: "http://127.0.0.1:11434/v1",
        suggested_model: None,
    },
    ClientLlmApiBasePreset {
        id: "deepseek",
        url: "https://api.deepseek.com/v1",
        suggested_model: Some("deepseek-chat"),
    },
    ClientLlmApiBasePreset {
        id: "minimax",
        url: "https://api.minimaxi.com/v1",
        suggested_model: None,
    },
    ClientLlmApiBasePreset {
        id: "zhipu",
        url: "https://open.bigmodel.cn/api/paas/v4",
        suggested_model: Some("glm-5"),
    },
    ClientLlmApiBasePreset {
        id: "moonshot",
        url: "https://api.moonshot.cn/v1",
        suggested_model: Some("kimi-k2.5"),
    },
    ClientLlmApiBasePreset {
        id: "custom",
        url: "",
        suggested_model: None,
    },
];

pub fn preset_by_id(id: &str) -> Option<&'static ClientLlmApiBasePreset> {
    CLIENT_LLM_API_BASE_PRESETS.iter().find(|p| p.id == id)
}

/// 当前草稿对应的 `<select>` value：`preset.id`，或与任一预设 `url` 完全一致时取该预设 id，否则 `custom`。
pub fn api_base_select_value_for_draft(draft: &str) -> &'static str {
    let t = draft.trim();
    if t.is_empty() {
        return "server";
    }
    for p in CLIENT_LLM_API_BASE_PRESETS {
        if p.id == "server" || p.id == "custom" {
            continue;
        }
        if p.url == t {
            return p.id;
        }
    }
    "custom"
}

#[cfg(test)]
mod tests {
    use super::api_base_select_value_for_draft;

    #[test]
    fn api_base_select_value_empty_is_server() {
        assert_eq!(api_base_select_value_for_draft(""), "server");
    }

    #[test]
    fn api_base_select_value_matches_preset_url() {
        assert_eq!(
            api_base_select_value_for_draft("https://api.deepseek.com/v1"),
            "deepseek"
        );
        assert_eq!(
            api_base_select_value_for_draft("http://127.0.0.1:11434/v1"),
            "ollama"
        );
    }

    #[test]
    fn api_base_select_value_unknown_is_custom() {
        assert_eq!(
            api_base_select_value_for_draft("https://example.com/v1"),
            "custom"
        );
    }
}
