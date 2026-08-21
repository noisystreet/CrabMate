//! OpenAI 兼容网关 URL / 建议模型预设表（文档示例；`resolve_api_base_set_arg` 供历史 CLI 辅助）。
//!
//! 官方 Web 设置页另有一份 UI 拷贝：Client `frontend/src/client_llm_presets.rs`。
//! 出站匹配、能力与**完整常用模型列表**以 Agent **`config/llm_vendors.toml`** 为准（本表 `suggested_model` 应对应该厂商 `models` 首项）。

/// 单条预设：`id` 用于 UI / slash 补全；`url` 写入 `api_base`（空串表示「沿用服务端 / 自定义」占位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LlmApiBasePreset {
    pub id: &'static str,
    pub url: &'static str,
    /// 选此项时，若模型名为空则填入建议值（不覆盖用户已填的 model）。
    pub suggested_model: Option<&'static str>,
}

/// 常用 OpenAI 兼容网关（与 README / 配置说明中的供应商示例一致）。
pub const LLM_API_BASE_PRESETS: &[LlmApiBasePreset] = &[
    LlmApiBasePreset {
        id: "server",
        url: "",
        suggested_model: None,
    },
    LlmApiBasePreset {
        id: "ollama",
        url: "http://127.0.0.1:11434/v1",
        suggested_model: None,
    },
    LlmApiBasePreset {
        id: "deepseek",
        url: "https://api.deepseek.com/v1",
        suggested_model: Some("deepseek-v4-flash"),
    },
    LlmApiBasePreset {
        id: "minimax",
        url: "https://api.minimaxi.com/v1",
        suggested_model: None,
    },
    LlmApiBasePreset {
        id: "zhipu",
        url: "https://open.bigmodel.cn/api/paas/v4",
        suggested_model: Some("glm-5.3"),
    },
    LlmApiBasePreset {
        id: "moonshot",
        url: "https://api.moonshot.cn/v1",
        suggested_model: Some("kimi-k3"),
    },
    LlmApiBasePreset {
        id: "custom",
        url: "",
        suggested_model: None,
    },
];

/// 当前草稿对应的预设 id：空 → `server`；与任一非 server/custom 预设 `url` 完全一致 → 该 id；否则 `custom`。
pub fn api_base_select_value_for_draft(draft: &str) -> &'static str {
    let t = draft.trim();
    if t.is_empty() {
        return "server";
    }
    for p in LLM_API_BASE_PRESETS {
        if p.id == "server" || p.id == "custom" {
            continue;
        }
        if p.url == t {
            return p.id;
        }
    }
    "custom"
}

/// 按预设 id（大小写不敏感）查找；`server` / `custom` 也返回（url 可能为空）。
pub fn llm_api_base_preset_by_id(id: &str) -> Option<&'static LlmApiBasePreset> {
    let t = id.trim();
    if t.is_empty() {
        return None;
    }
    LLM_API_BASE_PRESETS
        .iter()
        .find(|p| p.id.eq_ignore_ascii_case(t))
}

/// 将 `/api-base set` 参数解析为网关 URL：命中带非空 `url` 的预设 id 则用其 URL，否则视为字面 URL。
///
/// 返回 `(api_base, suggested_model)`。`server` / `custom` 无 URL，返回 `None`。
pub fn resolve_api_base_set_arg(arg: &str) -> Option<(String, Option<&'static str>)> {
    let t = arg.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(p) = llm_api_base_preset_by_id(t) {
        if p.url.is_empty() {
            return None;
        }
        return Some((p.url.to_string(), p.suggested_model));
    }
    Some((t.to_string(), None))
}

/// 有非空 URL 的预设（供 REPL 列表与补全；排除 `server` / `custom`）。
pub fn llm_api_base_presets_with_url() -> impl Iterator<Item = &'static LlmApiBasePreset> {
    LLM_API_BASE_PRESETS.iter().filter(|p| !p.url.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn resolve_preset_id_and_literal_url() {
        let (url, model) = resolve_api_base_set_arg("deepseek").unwrap();
        assert_eq!(url, "https://api.deepseek.com/v1");
        assert_eq!(model, Some("deepseek-v4-flash"));
        assert!(resolve_api_base_set_arg("server").is_none());
        let (u2, m2) = resolve_api_base_set_arg("https://example.com/v1").unwrap();
        assert_eq!(u2, "https://example.com/v1");
        assert!(m2.is_none());
    }
}
