//! 嵌入 **`config/llm_vendors.toml`**：按 `api_base` / 模型 ID 匹配厂商，并给出出站能力与常用 `models` 列表。

use std::sync::OnceLock;

use serde::Deserialize;

const EMBEDDED_LLM_VENDORS: &str = include_str!("../../config/llm_vendors.toml");

/// 与 TOML **`adapter`** 对应的 Rust 适配器族。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VendorAdapterId {
    Generic,
    Deepseek,
    Kimi,
    Glm,
    Minimax,
}

/// 匹配到的厂商出站能力（模型级 `model_caps` 已叠加上）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedVendorCaps {
    pub adapter: VendorAdapterId,
    pub fold_system_into_user: bool,
    pub default_reasoning_split: bool,
    pub explicit_cache_control: bool,
    pub image_url_content_parts: bool,
}

impl ResolvedVendorCaps {
    const UNMATCHED: Self = Self {
        adapter: VendorAdapterId::Generic,
        fold_system_into_user: false,
        default_reasoning_split: false,
        explicit_cache_control: false,
        image_url_content_parts: true,
    };
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VendorsFile {
    vendor: Vec<VendorEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VendorEntry {
    id: String,
    adapter: VendorAdapterId,
    #[serde(default)]
    api_base_contains: Vec<String>,
    #[serde(default)]
    model_id_prefixes: Vec<String>,
    /// 规范网关 URL（文档 / 后续 API 暴露；匹配不依赖本字段）。
    #[serde(default)]
    #[allow(dead_code)]
    canonical_api_bases: Vec<String>,
    #[serde(default)]
    models: Vec<String>,
    #[serde(default)]
    fold_system_into_user: bool,
    #[serde(default)]
    default_reasoning_split: bool,
    #[serde(default)]
    explicit_cache_control: bool,
    #[serde(default = "default_true")]
    image_url_content_parts: bool,
    #[serde(default)]
    model_caps: Vec<ModelCapsEntry>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelCapsEntry {
    #[serde(default)]
    id_equals: Vec<String>,
    #[serde(default)]
    id_contains: Vec<String>,
    fold_system_into_user: Option<bool>,
    default_reasoning_split: Option<bool>,
    explicit_cache_control: Option<bool>,
    image_url_content_parts: Option<bool>,
}

#[derive(Debug)]
struct VendorCatalog {
    vendors: Vec<VendorEntry>,
}

static CATALOG: OnceLock<VendorCatalog> = OnceLock::new();

fn parse_vendors_file(src: &str) -> Result<VendorCatalog, String> {
    let file: VendorsFile =
        toml::from_str(src).map_err(|e| format!("llm_vendors.toml: {e}"))?;
    if file.vendor.is_empty() {
        return Err("llm_vendors.toml: [[vendor]] 不能为空".to_string());
    }
    for v in &file.vendor {
        if v.id.trim().is_empty() {
            return Err("llm_vendors.toml: vendor.id 不能为空".to_string());
        }
        if v.api_base_contains.is_empty() && v.model_id_prefixes.is_empty() {
            return Err(format!(
                "llm_vendors.toml: vendor `{}` 须至少有 api_base_contains 或 model_id_prefixes",
                v.id
            ));
        }
    }
    Ok(VendorCatalog {
        vendors: file.vendor,
    })
}

fn catalog() -> &'static VendorCatalog {
    CATALOG.get_or_init(|| {
        parse_vendors_file(EMBEDDED_LLM_VENDORS)
            .unwrap_or_else(|e| panic!("embedded config/llm_vendors.toml invalid: {e}"))
    })
}

fn ascii_contains(hay: &str, needle: &str) -> bool {
    hay.to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn ascii_starts_with(hay: &str, prefix: &str) -> bool {
    hay.to_ascii_lowercase()
        .starts_with(&prefix.to_ascii_lowercase())
}

fn vendor_matches(v: &VendorEntry, model: &str, api_base: &str) -> bool {
    let prefix_hit = v
        .model_id_prefixes
        .iter()
        .any(|p| !p.is_empty() && ascii_starts_with(model, p));
    let base_hit = v
        .api_base_contains
        .iter()
        .any(|s| !s.is_empty() && ascii_contains(api_base, s));
    prefix_hit || base_hit
}

fn model_caps_hit(cap: &ModelCapsEntry, model: &str) -> bool {
    let eq = cap
        .id_equals
        .iter()
        .any(|id| !id.is_empty() && id.eq_ignore_ascii_case(model.trim()));
    let sub = cap
        .id_contains
        .iter()
        .any(|frag| !frag.is_empty() && ascii_contains(model, frag));
    eq || sub
}

fn caps_from_vendor(v: &VendorEntry, model: &str) -> ResolvedVendorCaps {
    let mut caps = ResolvedVendorCaps {
        adapter: v.adapter,
        fold_system_into_user: v.fold_system_into_user,
        default_reasoning_split: v.default_reasoning_split,
        explicit_cache_control: v.explicit_cache_control,
        image_url_content_parts: v.image_url_content_parts,
    };
    for cap in &v.model_caps {
        if !model_caps_hit(cap, model) {
            continue;
        }
        if let Some(x) = cap.fold_system_into_user {
            caps.fold_system_into_user = x;
        }
        if let Some(x) = cap.default_reasoning_split {
            caps.default_reasoning_split = x;
        }
        if let Some(x) = cap.explicit_cache_control {
            caps.explicit_cache_control = x;
        }
        if let Some(x) = cap.image_url_content_parts {
            caps.image_url_content_parts = x;
        }
    }
    caps
}

fn find_vendor<'a>(cat: &'a VendorCatalog, model: &str, api_base: &str) -> Option<&'a VendorEntry> {
    cat.vendors.iter().find(|v| vendor_matches(v, model, api_base))
}

/// 按目录匹配当前 **`model` + `api_base`**（无命中则 OpenAI 兼容缺省：可发 `image_url`）。
#[must_use]
pub fn resolved_vendor_caps(model: &str, api_base: &str) -> ResolvedVendorCaps {
    match find_vendor(catalog(), model, api_base) {
        Some(v) => caps_from_vendor(v, model),
        None => ResolvedVendorCaps::UNMATCHED,
    }
}

/// 命中的厂商 **`id`**（如 **`deepseek`**）；未命中为 **`None`**。
#[must_use]
pub fn matched_vendor_id(model: &str, api_base: &str) -> Option<&'static str> {
    find_vendor(catalog(), model, api_base).map(|v| v.id.as_str())
}

/// 厂商常用模型 ID 列表（配置里的 **`models`**；未命中为空切片）。
#[must_use]
pub fn matched_vendor_models(model: &str, api_base: &str) -> &'static [String] {
    find_vendor(catalog(), model, api_base)
        .map(|v| v.models.as_slice())
        .unwrap_or(&[])
}

/// 建议默认模型：命中厂商 **`models` 首项**。
#[must_use]
pub fn matched_vendor_default_model(model: &str, api_base: &str) -> Option<&'static str> {
    matched_vendor_models(model, api_base)
        .first()
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_parses() {
        assert!(!catalog().vendors.is_empty());
    }

    #[test]
    fn volcano_beats_kimi_model_prefix() {
        let caps = resolved_vendor_caps(
            "Kimi-K2.6",
            "https://ark.cn-beijing.volces.com/api/coding/v3",
        );
        assert_eq!(caps.adapter, VendorAdapterId::Generic);
        assert_eq!(
            matched_vendor_id("Kimi-K2.6", "https://ark.cn-beijing.volces.com/api/coding/v3"),
            Some("volcano")
        );
    }

    #[test]
    fn kimi_models_list_and_adapter() {
        let caps = resolved_vendor_caps("kimi-k3", "https://api.moonshot.cn/v1");
        assert_eq!(caps.adapter, VendorAdapterId::Kimi);
        assert!(caps.image_url_content_parts);
        let models = matched_vendor_models("kimi-k3", "https://api.moonshot.cn/v1");
        assert_eq!(models[0], "kimi-k3");
        assert!(models.iter().any(|m| m == "kimi-k2.6"));
        let code = resolved_vendor_caps("kimi-k2.7-code", "https://api.moonshot.cn/v1");
        assert!(!code.image_url_content_parts);
    }

    #[test]
    fn minimax_fold_and_no_image_url() {
        let caps = resolved_vendor_caps("MiniMax-M2.7", "https://api.minimaxi.com/v1");
        assert_eq!(caps.adapter, VendorAdapterId::Minimax);
        assert!(caps.fold_system_into_user);
        assert!(caps.default_reasoning_split);
        assert!(!caps.image_url_content_parts);
        let m3 = resolved_vendor_caps("MiniMax-M3", "https://api.minimax.io/v1");
        assert!(m3.image_url_content_parts);
        let models = matched_vendor_models("MiniMax-M3", "https://api.minimaxi.com/v1");
        assert_eq!(models[0], "MiniMax-M3");
    }

    #[test]
    fn deepseek_text_vs_vision_image_url() {
        let flash = resolved_vendor_caps("deepseek-v4-flash", "https://api.deepseek.com/v1");
        assert_eq!(flash.adapter, VendorAdapterId::Deepseek);
        assert!(!flash.image_url_content_parts);
        assert!(flash.explicit_cache_control);
        let vision = resolved_vendor_caps(
            "deepseek-v4-flash-vision-exp",
            "https://api.deepseek.com/v1",
        );
        assert!(vision.image_url_content_parts);
        let models = matched_vendor_models("deepseek-v4-flash", "https://api.deepseek.com/v1");
        assert_eq!(models[0], "deepseek-v4-flash");
        assert!(models.iter().any(|m| m == "deepseek-v4-flash-vision-exp"));
        assert!(!models.iter().any(|m| m == "deepseek-chat"));
    }

    #[test]
    fn zhipu_text_vs_vision_and_suggested_model() {
        let text = resolved_vendor_caps("glm-5.3", "https://open.bigmodel.cn/api/paas/v4");
        assert_eq!(text.adapter, VendorAdapterId::Glm);
        assert!(!text.image_url_content_parts);
        let vision = resolved_vendor_caps("glm-5v-turbo", "https://open.bigmodel.cn/api/paas/v4");
        assert!(vision.image_url_content_parts);
        let models = matched_vendor_models("glm-5.3", "https://open.bigmodel.cn/api/paas/v4");
        assert_eq!(models[0], "glm-5.3");
    }

    #[test]
    fn unmatched_keeps_image_url() {
        let caps = resolved_vendor_caps("gpt-4o", "https://api.openai.com/v1");
        assert_eq!(caps.adapter, VendorAdapterId::Generic);
        assert!(caps.image_url_content_parts);
        assert!(matched_vendor_id("gpt-4o", "https://api.openai.com/v1").is_none());
    }

    #[test]
    fn canonical_api_bases_listed_for_deepseek() {
        let v = catalog()
            .vendors
            .iter()
            .find(|v| v.id == "deepseek")
            .expect("deepseek");
        assert!(
            v.canonical_api_bases
                .iter()
                .any(|u| u == "https://api.deepseek.com/v1")
        );
    }
}
