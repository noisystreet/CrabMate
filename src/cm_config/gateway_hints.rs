//! 按 **`model` / `api_base`** 推断网关族行为（权威表 **`config/llm_vendors.toml`**，经 [`crate::cm_llm::vendor_catalog`]）。

/// TOML/环境变量均未设置 **`llm_reasoning_split`** 时的默认值（目录 **`default_reasoning_split`**）。
#[inline]
pub fn default_llm_reasoning_split_for_gateway(model: &str, api_base: &str) -> bool {
    crate::cm_llm::vendor_catalog::resolved_vendor_caps(model, api_base).default_reasoning_split
}

/// 出站是否将独立 **`system`** 折叠进 **`user`**（目录 **`fold_system_into_user`**）。
#[inline]
pub fn fold_system_into_user_for_config(model: &str, api_base: &str) -> bool {
    crate::cm_llm::vendor_catalog::resolved_vendor_caps(model, api_base).fold_system_into_user
}

#[cfg(test)]
mod tests {
    use super::{default_llm_reasoning_split_for_gateway, fold_system_into_user_for_config};
    use crate::cm_types::llm_config as legacy;

    /// `cm_types::llm_config` 里还留着一份 **MiniMax 硬编码**的同名逻辑（生产零调用，只剩自测）。
    /// 本仓依赖方向是 `cm_config → cm_llm → cm_types`，`cm_types` 够不到目录，所以那份删不掉、
    /// 也没法委托——只能**钉住两边口径**：目录改了而那份没跟上（或反之）会在这里变红。
    ///
    /// 覆盖 MiniMax 的两种命中方式（模型前缀 / api_base 子串）、以及几个不相关的网关。
    #[test]
    fn catalog_matches_the_legacy_cm_types_hints() {
        let cases = [
            ("MiniMax-M2.7", "https://api.deepseek.com/v1"),
            ("some-id", "https://api.minimaxi.com/v1"),
            ("deepseek-chat", "https://api.deepseek.com/v1"),
            ("glm-5", "https://open.bigmodel.cn/api/paas/v4"),
            ("kimi-k2.5", "https://api.moonshot.cn/v1"),
            ("unknown-model", "http://127.0.0.1:11434/v1"),
        ];
        for (model, base) in cases {
            assert_eq!(
                default_llm_reasoning_split_for_gateway(model, base),
                legacy::default_llm_reasoning_split_for_gateway(model, base),
                "reasoning_split 口径与 cm_types 副本漂移：{model} @ {base}"
            );
            assert_eq!(
                fold_system_into_user_for_config(model, base),
                legacy::fold_system_into_user_for_gateway(model, base),
                "fold_system_into_user 口径与 cm_types 副本漂移：{model} @ {base}"
            );
        }
    }
}
