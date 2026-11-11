//! `context_trim` / `context_inject` 的 `detail` 类型（#1347 S4a）。
//!
//! **为什么是结构体而不是 `json!` 手写**：字段集合、类型、默认值都变成编译期约束；
//! 漏一个字段或改名会直接编译失败，不再靠「记得同步改这里」。
//!
//! **wire 契约（S4a 的边界）**：`detail` 在线上与 canonical 里仍是**字符串化 JSON**
//! （`TimelineLogBody.detail: Option<String>`，`crabmate_timeline` 旁注同理）。本模块只负责
//! 提供 [`ContextTimelineDetail::to_json_string`]，序列化仍在 encode 边界做。字段声明顺序
//! 即 JSON 键顺序，**改动顺序会让 `fixtures/context_timeline_detail_golden.json` 变红**——
//! 这是有意的：顺序漂移虽然线上无害，但会让「同一状态两种实现逐字相同」的断言失效。
//!
//! S4b（把 wire 改成 JSON object）不在本模块范围内，见 `docs/design/context_timeline_detail.md`。

use crate::cm_agent::message_pipeline::ToolCompressHit;

/// `context_trim` 的 `detail`。
///
/// 字段顺序即 JSON 键顺序，见模块文档。
///
/// `deny_unknown_fields` 不是为了严格——是为了让 `#[serde(untagged)]` 的判别**成立**：
/// 两个 struct 全字段有默认值，若允许未知字段，`{"kinds":…}` 会被 `Trim` 先吞掉并解析成
/// `Trim(default)`，`Inject` 分支永远不可达。代价是「未来给 detail 加字段后，旧版本代码
/// 解析新 JSON 会失败」——detail 由服务端自己写、字段集合同版本固定，且新代码读旧数据仍靠
/// `serde(default)` 补位，这个方向是对的。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TrimDetail {
    /// 条数闸是否命中（`max_message_history`）。
    pub count_hit: bool,
    /// 字符闸是否命中（仅在未配置窗口时生效）。
    pub char_hit: bool,
    /// 本回合首次 prepare 前的消息条数。
    pub n_before: usize,
    /// 最近一次 prepare 后的消息条数。
    pub n_after: usize,
    /// 本档 `tool` 正文压缩命中条数（累计）。
    pub compress_hits: usize,
    /// 被压缩 `tool` 正文明细（#1327 C 案），每回合至多 16 条（`MAX_TOOL_COMPRESS_DETAIL`）。
    pub tool_compressed: Vec<ToolCompressHit>,
    /// 本回合是否做过 LLM 摘要。
    pub summarized: bool,
    /// 本回合是否**真的**动过历史（#1345）：客户端只在 `true` 时回落用量水位。
    pub history_removed: bool,
    /// 摘要实际保留的尾部条数。
    pub tail_kept: Option<usize>,
    pub before_tokens: Option<u32>,
    pub after_tokens: Option<u32>,
    pub max_input_tokens: Option<u32>,
    pub reserved_output_tokens: Option<u32>,
    pub message_tokens: Option<u32>,
    pub tool_schema_tokens: Option<u32>,
    pub attachment_tokens: Option<u32>,
    /// `matched_tokenizer` / `fallback_tokenizer` / `conservative_bytes`；`None` 表示无估算。
    pub counting_source: Option<String>,
    pub token_triggered: bool,
    pub removed_turn_groups: usize,
    pub removed_messages: usize,
    /// `token_budget_summary` / `token_budget_turn_groups` / `token_budget_warning` / `none`。
    pub compaction_reason: Option<String>,
}

/// `context_inject` 的 `detail`。
///
/// `deny_unknown_fields` 的理由同 [`TrimDetail`]。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InjectDetail {
    /// 本轮注入的上下文种类（`workspace_profile` / `memory` / `changelist` / `execution_constraint`）。
    pub kinds: Vec<String>,
    /// 本轮系统提示里出现的技能 id。
    pub skills: Vec<String>,
    /// 是否由用户显式选用技能（`/<skill-id>`）。
    pub forced: bool,
}

/// 两类 `detail` 的判别联合。
///
/// `untagged` 是为了让 wire 保持「字符串化 JSON 对象」——detail 里不含 `kind`，
/// 判别只能靠字段集合。**判别能成立的前提是两个 struct 都 `deny_unknown_fields`**：
/// 它们全字段有默认值，所以「允许未知字段」时第一个变体会无条件吞下任何对象。
/// 加了 `deny_unknown_fields` 后，Trim 侧遇到 `kinds`/`skills`/`forced` 直接失败、
/// Inject 侧遇到 token 字段直接失败，互斥成立（`{}` 仍落到 `Trim`，空 detail 无意义，可接受）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ContextTimelineDetail {
    Trim(TrimDetail),
    Inject(InjectDetail),
}

impl ContextTimelineDetail {
    /// encode 边界的唯一出口：SSE `TimelineLogBody.detail` 与 `crabmate_timeline` 旁注都用它。
    ///
    /// 解析失败只可能来自实现错误（序列化 struct 不会失败），故回退为空对象字符串而非 panic——
    /// 与既有 `json!` 路径一样，宁可丢掉 `detail` 也不能让回合失败。
    #[must_use]
    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// 供日志/测试读取已序列化形态。
    #[must_use]
    pub fn as_trim(&self) -> Option<&TrimDetail> {
        match self {
            Self::Trim(d) => Some(d),
            Self::Inject(_) => None,
        }
    }

    /// 见 [`Self::as_trim`]。
    #[must_use]
    pub fn as_inject(&self) -> Option<&InjectDetail> {
        match self {
            Self::Inject(d) => Some(d),
            Self::Trim(_) => None,
        }
    }
}
