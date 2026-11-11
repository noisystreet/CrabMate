//! 配置层优先级：顺序表 + `key → 最终来源` 溯源（#1342 P1/P2）。
//!
//! **为什么存在**：配置有 5 层覆盖（内嵌 → 用户 TOML → 环境变量 → user-data → CLI），
//! 而优先级长期**就是代码里的调用顺序**——没有任何东西钉住「谁最终赢」，于是
//! `--llm-context-tokens` 被 user-data 静默盖掉（#1340）这类问题只能靠人肉审调用序列发现。
//!
//! 这里做两件事：
//!
//! 1. [`LAYERS`] 是层顺序的**唯一**表述；`cm_config::load::load_config_traced` 负责
//!    [`is_pre_load_layer`] 的三层，[`apply_post_load_layers`] 负责其余两层。
//! 2. [`LayerTrace`] 记录关键键（[`WatchedKey`]）**最后被哪一层改动**，供 `crabmate doctor`
//!    与 `fixtures/config_layer_precedence.json` 断言。
//!
//! 溯源的粒度刻意是「最后**改变取值**的层」：某层把键重新写成与上一层相同的值时不记录，
//! 因为生效结果相同、记谁都没有解释力。密钥类键只记「已设置/未设置」，从不记明文。

use std::sync::Arc;

use crate::cm_config::AgentConfig;
use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::ExposeSecret;

/// 配置覆盖层，声明顺序即由低到高的生效顺序。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigLayer {
    /// 内嵌 `config/*.toml` 分片。
    Embedded,
    /// 用户 `config.toml` / `--config` 指定的 TOML。
    UserToml,
    /// `CM_*` 环境变量。
    Env,
    /// user-data 目录下的 `llm_overrides.json` / `tool_overrides.json` / 钥匙串。
    UserData,
    /// 命令行显式覆盖（目前只有 `--llm-context-tokens`）。
    Cli,
}

impl ConfigLayer {
    /// 日志 / doctor / golden 里的稳定标识（**不要**翻译，测试与文档按它对齐）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::UserToml => "user_toml",
            Self::Env => "env",
            Self::UserData => "user_data",
            Self::Cli => "cli",
        }
    }

    /// 由标识还原（golden 里用字符串声明层）。
    #[must_use]
    pub fn from_str_exact(raw: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|layer| layer.as_str() == raw)
    }

    /// 全部层（由低到高），与 [`LAYERS`] 同序。
    pub const ALL: &'static [Self] = &[
        Self::Embedded,
        Self::UserToml,
        Self::Env,
        Self::UserData,
        Self::Cli,
    ];
}

/// **唯一**的层顺序表：字面量在 [`ConfigLayer::ALL`]，这里只是它的别名。
/// 调整顺序只改那一处，不要在调用点手工排 `apply_*`。
///
/// 覆盖它的两个执行点各按同一张表走：
/// - [`is_pre_load_layer`] 的层由 `cm_config::load::load_config_traced` 内部按序执行，
///   因为它们在同一个 builder 上合并（见 `cm_config::assembly`）；
/// - 其余层由 [`apply_post_load_layers`] 在最终 `AgentConfig` 上按序执行。
pub const LAYERS: &[ConfigLayer] = ConfigLayer::ALL;

/// 在 builder 阶段合并的层（内嵌分片 / 用户 TOML / 环境变量）。
#[must_use]
pub const fn is_pre_load_layer(layer: ConfigLayer) -> bool {
    matches!(
        layer,
        ConfigLayer::Embedded | ConfigLayer::UserToml | ConfigLayer::Env
    )
}

/// 在最终 `AgentConfig` 上叠加的层（user-data / CLI）。
#[must_use]
pub const fn is_post_load_layer(layer: ConfigLayer) -> bool {
    !is_pre_load_layer(layer)
}

/// 参与溯源的关键键：与 `fixtures/config_layer_precedence.json` 的矩阵口径一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WatchedKey {
    LlmContextTokens,
    MaxTokens,
    MaxMessageHistory,
    DisabledTools,
    WebApiBearer,
}

impl WatchedKey {
    /// 全部受监视键（golden 与 doctor 都按此顺序输出）。
    pub const ALL: &'static [Self] = &[
        Self::LlmContextTokens,
        Self::MaxTokens,
        Self::MaxMessageHistory,
        Self::DisabledTools,
        Self::WebApiBearer,
    ];

    /// 稳定标识（= 配置键名 / CLI flag 名）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LlmContextTokens => "llm_context_tokens",
            Self::MaxTokens => "max_tokens",
            Self::MaxMessageHistory => "max_message_history",
            Self::DisabledTools => "tool_registry_disabled_tools",
            Self::WebApiBearer => "web_api_bearer_token",
        }
    }

    #[must_use]
    pub fn from_str_exact(raw: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|key| key.as_str() == raw)
    }
}

/// 受监视键在某一时刻的取值快照。
///
/// 只用于**比较是否变化**与打印，不参与任何配置逻辑。密钥以「是否已设置」参与比较：
/// 两层的 bearer 都非空但内容不同时记为未变化——doctor 从不打印该值，区分它们没有意义，
/// 也不该为判断差异把明文在两个结构里各存一份。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct WatchedValues {
    llm_context_tokens: Option<u64>,
    max_tokens: Option<u64>,
    max_message_history: Option<u64>,
    disabled_tools: Option<Vec<String>>,
    web_api_bearer_set: Option<bool>,
}

impl WatchedValues {
    /// 从 builder 读（`None` = 该层尚未提供该键）。
    #[must_use]
    pub(super) fn from_builder(builder: &ConfigBuilder) -> Self {
        Self {
            llm_context_tokens: builder.llm_sampling.llm_context_tokens,
            max_tokens: builder.llm_sampling.max_tokens,
            max_message_history: builder.session_ui.max_message_history,
            disabled_tools: builder
                .tool_registry_policy
                .tool_registry_disabled_tools
                .clone(),
            web_api_bearer_set: builder
                .web_api
                .web_api_bearer_token
                .as_deref()
                .map(|token| !token.trim().is_empty()),
        }
    }

    /// 从最终 cfg 读（恒为 `Some`，便于与 builder 快照逐键比较）。
    #[must_use]
    pub(super) fn from_config(cfg: &AgentConfig) -> Self {
        Self {
            llm_context_tokens: Some(u64::from(cfg.llm_sampling.llm_context_tokens)),
            max_tokens: Some(u64::from(cfg.llm_sampling.max_tokens)),
            max_message_history: Some(
                u64::try_from(cfg.session_ui.max_message_history).unwrap_or(u64::MAX),
            ),
            disabled_tools: Some(disabled_tools_snapshot(
                cfg.tool_registry_policy.tool_registry_disabled_tools.as_ref(),
            )),
            web_api_bearer_set: Some(
                !cfg.web_api
                    .web_api_bearer_token
                    .expose_secret()
                    .trim()
                    .is_empty(),
            ),
        }
    }

    /// 把相对 `prev` **发生变化**的键登记到 `trace` 上（`layer` 为本次改动来源）。
    pub(super) fn record_changes(&self, prev: &Self, layer: ConfigLayer, trace: &mut LayerTrace) {
        let pairs: [(&WatchedKey, bool); 5] = [
            (
                &WatchedKey::LlmContextTokens,
                self.llm_context_tokens != prev.llm_context_tokens,
            ),
            (
                &WatchedKey::MaxTokens,
                self.max_tokens != prev.max_tokens,
            ),
            (
                &WatchedKey::MaxMessageHistory,
                self.max_message_history != prev.max_message_history,
            ),
            (
                &WatchedKey::DisabledTools,
                self.disabled_tools != prev.disabled_tools,
            ),
            (
                &WatchedKey::WebApiBearer,
                self.web_api_bearer_set != prev.web_api_bearer_set,
            ),
        ];
        for (key, changed) in pairs {
            // bearer 只把「设成非空」当有效改动：清空/沿用空值不值得在溯源里占一行。
            let meaningful = *key != WatchedKey::WebApiBearer
                || self.web_api_bearer_set == Some(true);
            if changed && meaningful {
                trace.record(*key, layer);
            }
        }
    }
}

fn sorted_names(set: &std::collections::HashSet<String>) -> Vec<String> {
    let mut names: Vec<String> = set.iter().cloned().collect();
    names.sort();
    names
}

/// `key → 最终来源` 溯源：每个关键键只留最后一次有效改动。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayerTrace {
    rows: Vec<(WatchedKey, ConfigLayer)>,
}

impl LayerTrace {
    /// 记录（覆盖）某键的来源。
    pub fn record(&mut self, key: WatchedKey, layer: ConfigLayer) {
        match self.rows.iter_mut().find(|(k, _)| *k == key) {
            Some(row) => row.1 = layer,
            None => self.rows.push((key, layer)),
        }
    }

    /// 某键的最终来源；`None` 表示没有任何层改动过它（沿用更低层的值）。
    #[must_use]
    pub fn source_of(&self, key: WatchedKey) -> Option<ConfigLayer> {
        self.rows
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, layer)| *layer)
    }

    /// 已记录的（键, 来源）对，顺序按首次记录。
    ///
    /// 只给测试/调试用；生产展示走 [`Self::doctor_lines`]，避免多一处需要保持格式的出口。
    #[must_use]
    #[cfg(test)]
    pub fn rows(&self) -> &[(WatchedKey, ConfigLayer)] {
        &self.rows
    }

    /// doctor 用的「值 + 来源」行；密钥只写「已设置/未设置」。
    ///
    /// 括号只出现一次：值描述自己不带括号（`未设置` / `N 个`），来源整体放进外层括号，
    /// 否则会叠成 `（未设置）（未覆盖（沿用内嵌默认））` 这种读不懂的行。
    #[must_use]
    pub fn doctor_lines(&self, cfg: &AgentConfig) -> Vec<String> {
        WatchedKey::ALL
            .iter()
            .map(|key| {
                let source = self
                    .source_of(*key)
                    .map_or("未覆盖，沿用内嵌默认".to_string(), |layer| {
                        format!("最后由 {}", layer.as_str())
                    });
                format!("  {} = {}（{source}）", key.as_str(), describe_value(cfg, *key))
            })
            .collect()
    }
}

fn describe_value(cfg: &AgentConfig, key: WatchedKey) -> String {
    match key {
        WatchedKey::LlmContextTokens => cfg.llm_sampling.llm_context_tokens.to_string(),
        WatchedKey::MaxTokens => cfg.llm_sampling.max_tokens.to_string(),
        WatchedKey::MaxMessageHistory => cfg.session_ui.max_message_history.to_string(),
        WatchedKey::DisabledTools => cfg
            .tool_registry_policy
            .tool_registry_disabled_tools
            .as_ref()
            .map_or_else(|| "未设置".to_string(), |set| format!("{} 个", set.len())),
        WatchedKey::WebApiBearer => {
            if cfg
                .web_api
                .web_api_bearer_token
                .expose_secret()
                .trim()
                .is_empty()
            {
                "未设置".to_string()
            } else {
                "已设置（值已隐藏）".to_string()
            }
        }
    }
}

/// 按 [`LAYERS`] 的**加载后**层序依次应用 `apply`，并在每层前后比较关键键取值记来源。
///
/// `apply` 只负责「这一层怎么写」，顺序与溯源都留在这里：把 CLI 覆盖挪到 user-data 之前
/// 必须动 [`LAYERS`]（或这里的筛选），而 `fixtures/config_layer_precedence.json` 会因此变红
/// —— 这正是 #1340 缺的那道闸。
pub fn apply_post_load_layers<F>(cfg: &mut AgentConfig, trace: &mut LayerTrace, mut apply: F)
where
    F: FnMut(ConfigLayer, &mut AgentConfig),
{
    for layer in LAYERS.iter().copied().filter(|l| is_post_load_layer(*l)) {
        let prev = WatchedValues::from_config(cfg);
        apply(layer, cfg);
        WatchedValues::from_config(cfg).record_changes(&prev, layer, trace);
    }
}

/// `tool_registry_disabled_tools` 的不可变集合形态转快照用名列表（测试与 doctor 共用）。
#[must_use]
pub fn disabled_tools_snapshot(set: Option<&Arc<std::collections::HashSet<String>>>) -> Vec<String> {
    set.map_or_else(Vec::new, |s| sorted_names(s.as_ref()))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};

    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        name: String,
        #[serde(default)]
        layers: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
        expected: BTreeMap<String, Expectation>,
    }

    #[derive(Deserialize)]
    struct Expectation {
        value: serde_json::Value,
        source: Option<String>,
    }

    fn default_cfg() -> AgentConfig {
        crate::cm_config::load_config(None).expect("embedded default config")
    }

    /// 层顺序表是契约：任何一格挪动都必须显式改这条断言（以及 golden）。
    #[test]
    fn layer_order_is_frozen() {
        let names: Vec<&str> = LAYERS.iter().map(|layer| layer.as_str()).collect();
        assert_eq!(
            names,
            vec!["embedded", "user_toml", "env", "user_data", "cli"]
        );
    }

    /// 前三个在 builder 阶段合并，后两个在最终 cfg 上叠加；两侧覆盖必须互补。
    #[test]
    fn pre_and_post_load_layers_partition_the_table() {
        for layer in LAYERS.iter().copied() {
            assert_ne!(
                is_pre_load_layer(layer),
                is_post_load_layer(layer),
                "{} 必须恰好属于一侧",
                layer.as_str()
            );
        }
        assert!(is_pre_load_layer(ConfigLayer::Env));
        assert!(is_post_load_layer(ConfigLayer::UserData));
        assert!(is_post_load_layer(ConfigLayer::Cli));
    }

    /// 更高层写入**与当前相同的值**不算改动：来源停在真正改变取值的那个层。
    #[test]
    fn equal_value_at_higher_layer_keeps_earlier_source() {
        let mut cfg = default_cfg();
        let mut trace = LayerTrace::default();
        let target = cfg.llm_sampling.llm_context_tokens.saturating_sub(1_000).max(1);
        apply_post_load_layers(&mut cfg, &mut trace, |layer, cfg| {
            if matches!(layer, ConfigLayer::UserData | ConfigLayer::Cli) {
                cfg.llm_sampling.llm_context_tokens = target;
            }
        });
        assert_eq!(cfg.llm_sampling.llm_context_tokens, target);
        assert_eq!(
            trace.source_of(WatchedKey::LlmContextTokens),
            Some(ConfigLayer::UserData),
            "cli 只是重复写了同值，来源应停在 user_data"
        );
    }

    /// 内嵌分片是**基线**而不是覆盖层：它写下的值不得在 trace 里留行。
    ///
    /// 否则 doctor 会把默认值一律说成「最后由 embedded」，而 golden 里
    /// `source: null`（沿用内嵌默认）这条口径就永远不会出现在真实输出里——两处各说一套。
    #[test]
    fn embedded_shards_leave_no_trace_rows() {
        let (_cfg, trace) =
            crate::cm_config::load_config_traced(None).expect("embedded default config load ok");
        assert!(
            trace
                .rows()
                .iter()
                .all(|(_, layer)| *layer != ConfigLayer::Embedded),
            "内嵌分片不得记成来源: {:?}",
            trace.rows()
        );
    }

    /// doctor 的溯源行绝不能出现 bearer 明文（AGENTS §4.5）。
    #[test]
    fn doctor_lines_never_print_bearer_secret() {
        let mut cfg = default_cfg();
        let mut trace = LayerTrace::default();
        apply_post_load_layers(&mut cfg, &mut trace, |layer, cfg| {
            if layer == ConfigLayer::UserData {
                cfg.web_api.web_api_bearer_token =
                    secrecy::SecretString::new("placeholder-bearer-xyzzy".into());
            }
        });
        let text = trace.doctor_lines(&cfg).join("\n");
        assert!(text.contains("已设置（值已隐藏）"), "{text}");
        assert!(!text.contains("xyzzy"), "溯源行泄露了密钥: {text}");
        assert!(text.contains("最后由 user_data"), "{text}");
    }

    fn u32_of(value: &serde_json::Value, key: WatchedKey) -> u32 {
        u32::try_from(value.as_u64().unwrap_or_else(|| panic!("{} 需要整数", key.as_str())))
            .unwrap_or_else(|_| panic!("{} 超出 u32", key.as_str()))
    }

    fn usize_of(value: &serde_json::Value, key: WatchedKey) -> usize {
        usize::try_from(value.as_u64().unwrap_or_else(|| panic!("{} 需要整数", key.as_str())))
            .unwrap_or_else(|_| panic!("{} 超出 usize", key.as_str()))
    }

    /// 按 fixture 声明的键值写入 cfg（模拟该层「改了这些键」）。
    fn set_watched(cfg: &mut AgentConfig, key: WatchedKey, value: &serde_json::Value) {
        match key {
            WatchedKey::LlmContextTokens => cfg.llm_sampling.llm_context_tokens = u32_of(value, key),
            WatchedKey::MaxTokens => cfg.llm_sampling.max_tokens = u32_of(value, key),
            WatchedKey::MaxMessageHistory => {
                cfg.session_ui.max_message_history = usize_of(value, key);
            }
            WatchedKey::DisabledTools => {
                let names: Vec<String> = value
                    .as_array()
                    .unwrap_or_else(|| panic!("{} 需要字符串数组", key.as_str()))
                    .iter()
                    .map(|item| {
                        item.as_str()
                            .unwrap_or_else(|| panic!("{} 元素需为字符串", key.as_str()))
                            .to_string()
                    })
                    .collect();
                cfg.tool_registry_policy.tool_registry_disabled_tools =
                    Some(Arc::new(names.into_iter().collect::<HashSet<String>>()));
            }
            WatchedKey::WebApiBearer => {
                let token = value
                    .as_str()
                    .unwrap_or_else(|| panic!("{} 需要字符串占位", key.as_str()));
                cfg.web_api.web_api_bearer_token =
                    secrecy::SecretString::new(token.to_string().into());
            }
        }
    }

    /// 当前取值（与 fixture 的 `value` 同一口径；bearer 只报 set/unset）。
    fn current_json(cfg: &AgentConfig, key: WatchedKey) -> serde_json::Value {
        match key {
            WatchedKey::LlmContextTokens => serde_json::json!(cfg.llm_sampling.llm_context_tokens),
            WatchedKey::MaxTokens => serde_json::json!(cfg.llm_sampling.max_tokens),
            WatchedKey::MaxMessageHistory => serde_json::json!(cfg.session_ui.max_message_history),
            WatchedKey::DisabledTools => serde_json::json!(disabled_tools_snapshot(
                cfg.tool_registry_policy.tool_registry_disabled_tools.as_ref()
            )),
            WatchedKey::WebApiBearer => {
                let set = !cfg
                    .web_api
                    .web_api_bearer_token
                    .expose_secret()
                    .trim()
                    .is_empty();
                serde_json::json!(if set { "set" } else { "unset" })
            }
        }
    }

    /// #1342 P1：层优先级 golden。把某层挪到另一层之前 / 之后，本断言必红。
    #[test]
    fn golden_layer_precedence_matrix_matches_fixture() {
        let fixture: Fixture =
            serde_json::from_str(include_str!("../../fixtures/config_layer_precedence.json"))
                .expect("parse fixtures/config_layer_precedence.json");
        assert!(!fixture.cases.is_empty(), "golden 不得为空");
        for case in fixture.cases {
            let mut cfg = default_cfg();
            let mut trace = LayerTrace::default();
            apply_post_load_layers(&mut cfg, &mut trace, |layer, cfg| {
                let Some(overrides) = case.layers.get(layer.as_str()) else {
                    return;
                };
                for (raw_key, value) in overrides {
                    let key = WatchedKey::from_str_exact(raw_key)
                        .unwrap_or_else(|| panic!("golden 里的未知键 {raw_key}"));
                    set_watched(cfg, key, value);
                }
            });
            for (raw_key, expectation) in &case.expected {
                let key = WatchedKey::from_str_exact(raw_key)
                    .unwrap_or_else(|| panic!("golden 里的未知键 {raw_key}"));
                assert_eq!(
                    current_json(&cfg, key),
                    expectation.value,
                    "case={} key={raw_key} 最终取值不符",
                    case.name
                );
                let expected_source = expectation
                    .source
                    .as_deref()
                    .map(|raw| {
                        ConfigLayer::from_str_exact(raw)
                            .unwrap_or_else(|| panic!("golden 里的未知层 {raw}"))
                    });
                assert_eq!(
                    trace.source_of(key),
                    expected_source,
                    "case={} key={raw_key} 来源层不符",
                    case.name
                );
            }
        }
    }
}
