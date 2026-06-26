//! 物化策略与分阶段无工具轮语义。

/// 与配置 **`materialize_deepseek_dsml_tool_calls`** 对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DsmlMaterializePolicy {
    #[default]
    /// 无可用原生 `tool_calls` 时从正文 DSML 物化。
    FallbackWhenNoNative,
    /// 不解析；强依赖 API `tool_calls`。
    Off,
}

impl DsmlMaterializePolicy {
    #[must_use]
    pub fn from_enabled(enabled: bool) -> Self {
        if enabled {
            Self::FallbackWhenNoNative
        } else {
            Self::Off
        }
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn is_enabled(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// 分阶段无工具规划轮：物化后是否保留 `tool_calls`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagedDsmlHandling {
    /// 物化以检测违规，随后清空（仅解析规划 JSON）。
    DetectOnly,
    /// 物化并计数（原生 + DSML），用于首轮 rewrite 触发判定。
    CountForRewrite,
}

/// 分阶段路径上检测到的 DSML / 原生 tool_calls 条数（物化前/后）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StagedDsmlScanResult {
    pub raw_native_count: usize,
    pub materialized_count: usize,
}

impl StagedDsmlScanResult {
    #[must_use]
    pub fn total_for_rewrite_trigger(self) -> usize {
        self.raw_native_count
            .saturating_add(self.materialized_count)
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn had_violation(self) -> bool {
        self.materialized_count > 0
    }
}
