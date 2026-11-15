//! 子 Agent 委托的**契约层**（#839 P1）。
//!
//! 本模块只放类型与 trait，**不**依赖 `agent` / `llm`：委托的实现（装配子回合、调 LLM）
//! 在 `crate::agent::delegation`，经 trait object 注入回来。这与
//! [`crate::cm_tools::memory_tool_host`] 是同一范式——用「能力对象」换取
//! 「`cm_tools` 不反向依赖上层」。
//!
//! **为什么不能是普通 `cm_tools` runner**：子回合必须调 LLM，而 `api_key` / `client`
//! 在工具执行层**刻意不存在**（`agent_turn/host/execute/**` 中 `api_key` 零命中）。
//! 做成 runner 只有两条路：把凭据塞进 `DispatchToolParams`（等于让所有工具都能读到 LLM
//! 凭据）、或在 runner 里另建一处凭据装配点。依据
//! `docs/design/subagent_delegation.md` §3。
//!
//! 本模块**不含**任何派生逻辑（只读工具集、深度判定）：那些依赖 `AgentConfig` 与工具定义表，
//! 属装配层职责，见设计 §9 Q4。

use async_trait::async_trait;
use serde::Serialize;

use crate::cm_tools::tool_result::ToolError;

/// 子任务终态（设计 §4.2）。
///
/// `ok` 由子回合自己的 `TurnOutcome` 映射，**不**由子 Agent 自述——设计 §9 Q5 的原话是
/// 「自述仅为参考信号」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubTaskStatus {
    /// 正常结束。
    Ok,
    /// 撞上任一硬上限（LLM 次数 / 工具次数 / 墙钟）。
    BudgetExhausted,
    /// 父回合取消，子随之收手。
    Cancelled,
    /// 权限 / 深度 / 配置拒绝，子回合未真正开始。
    Denied,
    /// 内部失败（LLM 调用失败、装配失败等）。
    Failed,
}

impl SubTaskStatus {
    /// 稳定短名（日志 / SSE / 金样；改名即契约变更）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::BudgetExhausted => "budget_exhausted",
            Self::Cancelled => "cancelled",
            Self::Denied => "denied",
            Self::Failed => "failed",
        }
    }

    /// 是否算「拿到了可用产出」。父回合据此决定 report 的呈现方式。
    #[must_use]
    pub const fn is_ok(self) -> bool {
        matches!(self, Self::Ok)
    }
}

/// 一次委托的硬上限（设计 §7）。三个闸**额外**叠在子回合自己的 `TurnBudgetCounter` 之上。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubTaskLimits {
    /// LLM 调用次数上限（含首次）。
    pub max_llm_calls: u32,
    /// 工具调用次数上限。
    pub max_tool_calls: u32,
    /// 墙钟上限（秒）。
    pub max_wall_secs: u64,
}

impl Default for SubTaskLimits {
    /// 与 `docs/design/subagent_delegation.md` §10 的 `[subagent]` 默认值一致。
    fn default() -> Self {
        Self {
            max_llm_calls: 8,
            max_tool_calls: 16,
            max_wall_secs: 300,
        }
    }
}

/// 一次有界子任务的输入。**`prompt` 是唯一入口**：父历史、工具日志、system 全文一概不传。
#[derive(Debug, Clone)]
pub struct SubTaskRequest {
    /// 任务描述。
    pub prompt: String,
    /// 调用方透传的第几层（`0` = 父回合直接派）。既进日志/追踪，也用于越权复核。
    pub depth: u8,
    /// 允许的工具名；`None` = 按只读性派生全集（设计 §9 Q4），`Some` 只能**收窄**。
    pub allowed_tools: Option<Vec<String>>,
    pub limits: SubTaskLimits,
    /// 回传正文的长度上限（字符数）；`0` = 不截断。
    pub max_report_chars: usize,
}

impl SubTaskRequest {
    /// `max_report_chars` 默认值（设计 §10）。
    pub const DEFAULT_MAX_REPORT_CHARS: usize = 4000;

    /// 空白 `prompt` 不该走到实现层——空任务会白烧一次 LLM 往返。
    #[must_use]
    pub fn prompt_is_usable(&self) -> bool {
        !self.prompt.trim().is_empty()
    }
}

/// 子回合的用量统计。**父账本不并入这些数字**（设计 §13：父子预算口径分离），
/// report 里另列。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct SubTaskStats {
    pub llm_calls: u32,
    pub tool_calls: u32,
    pub elapsed_secs: u64,
    /// Token 粗估（与 `TurnBudgetCounter::estimated_tokens` 同口径）。
    pub estimated_tokens: u64,
}

/// 回传给父回合的有界产物。**绝不**含子 messages 或工具原文（设计 §8「回传有界」）。
///
/// 构造只经 [`SubTaskReport::bounded`]：上限在那一处统一施加，`truncated` 由实现自己判定
/// 而不接受调用方自述——否则「摘要太长」会悄悄把子任务省下的上下文全还回去。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubTaskReport {
    pub status: SubTaskStatus,
    pub summary: String,
    /// 关键路径（工作区相对或引用），最多 [`Self::MAX_KEY_PATHS`] 条。
    pub key_paths: Vec<String>,
    pub stats: SubTaskStats,
    /// 是否发生过截断（`summary` 或 `key_paths`），让父回合知道产出不完整。
    pub truncated: bool,
    /// 与工具信封 `error_code` 对齐的短码；`ok` 时通常为 `None`。
    pub error_code: Option<String>,
}

impl SubTaskReport {
    /// `key_paths` 条数上限（设计 §4.2）。
    pub const MAX_KEY_PATHS: usize = 20;

    /// 唯一构造入口：施加 `summary` 与 `key_paths` 上限，并**自行判定** `truncated`。
    #[must_use]
    pub fn bounded(
        status: SubTaskStatus,
        summary: impl Into<String>,
        key_paths: Vec<String>,
        stats: SubTaskStats,
        max_summary_chars: usize,
    ) -> Self {
        let (summary, summary_truncated) = truncate_chars(&summary.into(), max_summary_chars);
        let key_paths_truncated = key_paths.len() > Self::MAX_KEY_PATHS;
        let mut key_paths = key_paths;
        key_paths.truncate(Self::MAX_KEY_PATHS);
        Self {
            status,
            summary,
            key_paths,
            stats,
            truncated: summary_truncated || key_paths_truncated,
            error_code: None,
        }
    }

    #[must_use]
    pub fn with_error_code(mut self, code: impl Into<String>) -> Self {
        self.error_code = Some(code.into());
        self
    }
}

/// 按 **Unicode scalar** 截断（不是字节），截断时追加省略号。
///
/// `max_chars == 0` 表示不限制（与配置项「`0` = 不限制」的惯例一致）。
/// 返回 `(新串, 是否截断)`。
fn truncate_chars(s: &str, max_chars: usize) -> (String, bool) {
    if max_chars == 0 {
        return (s.to_string(), false);
    }
    if s.chars().count() <= max_chars {
        return (s.to_string(), false);
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push('…');
    (out, true)
}

/// 子 Agent 委托的执行面：由 `crate::agent::delegation` 实现并注入。
///
/// 实现侧必须保证：子回合用**全新 messages**、不写父 SSE 流、共享父的取消标志、
/// 权限取 `min` 链、不发起后台长任务（设计 §5）。本 trait 只固定输入输出形状。
#[async_trait]
pub trait SubAgentDelegator: Send + Sync {
    /// 跑一个有界子任务。**父回合无论如何都要能正常结束**（设计 §7）：
    /// 子任务被掐断只体现为一条 `status = budget_exhausted` 的 report。
    #[allow(clippy::result_large_err)] // `ToolError` 含正文解析快照，与其它工具宿主一致
    async fn run(&self, req: SubTaskRequest) -> Result<SubTaskReport, ToolError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> SubTaskStats {
        SubTaskStats {
            llm_calls: 3,
            tool_calls: 5,
            elapsed_secs: 12,
            estimated_tokens: 1234,
        }
    }

    #[test]
    fn status_strings_are_stable() {
        assert_eq!(SubTaskStatus::Ok.as_str(), "ok");
        assert_eq!(SubTaskStatus::BudgetExhausted.as_str(), "budget_exhausted");
        assert_eq!(SubTaskStatus::Cancelled.as_str(), "cancelled");
        assert_eq!(SubTaskStatus::Denied.as_str(), "denied");
        assert_eq!(SubTaskStatus::Failed.as_str(), "failed");
        assert!(SubTaskStatus::Ok.is_ok());
        assert!(!SubTaskStatus::Failed.is_ok());
    }

    #[test]
    fn limits_default_matches_design() {
        let d = SubTaskLimits::default();
        assert_eq!(d.max_llm_calls, 8);
        assert_eq!(d.max_tool_calls, 16);
        assert_eq!(d.max_wall_secs, 300);
        assert_eq!(SubTaskRequest::DEFAULT_MAX_REPORT_CHARS, 4000);
    }

    #[test]
    fn report_within_bounds_is_not_truncated() {
        let r = SubTaskReport::bounded(
            SubTaskStatus::Ok,
            "短摘要",
            vec!["src/a.rs".to_string()],
            stats(),
            100,
        );
        assert!(!r.truncated);
        assert_eq!(r.summary, "短摘要");
        assert_eq!(r.key_paths.len(), 1);
        assert_eq!(r.error_code, None);
    }

    #[test]
    fn long_summary_is_truncated_and_flagged() {
        let long = "问".repeat(50);
        let r = SubTaskReport::bounded(SubTaskStatus::Ok, long.clone(), Vec::new(), stats(), 10);
        assert!(r.truncated);
        // 10 个字符 + 省略号
        assert_eq!(r.summary.chars().count(), 11);
        assert!(r.summary.ends_with('…'));
    }

    #[test]
    fn key_paths_are_capped_at_max() {
        let paths: Vec<String> = (0..30).map(|i| format!("src/f{i}.rs")).collect();
        let r = SubTaskReport::bounded(SubTaskStatus::Ok, "s", paths, stats(), 100);
        assert_eq!(r.key_paths.len(), SubTaskReport::MAX_KEY_PATHS);
        assert!(r.truncated);
        // 截断保留的是前 N 条，不重排
        assert_eq!(r.key_paths[0], "src/f0.rs");
    }

    #[test]
    fn zero_max_chars_means_unlimited() {
        let long = "字".repeat(500);
        let r = SubTaskReport::bounded(SubTaskStatus::Ok, long, Vec::new(), stats(), 0);
        assert!(!r.truncated);
        assert_eq!(r.summary.chars().count(), 500);
    }

    #[test]
    fn truncation_is_unicode_safe() {
        // 多字节字符按 scalar 截断，不会切出半个 UTF-8 序列。
        let s = "äöü中文🙂abc";
        let r = SubTaskReport::bounded(SubTaskStatus::Ok, s, Vec::new(), stats(), 5);
        assert!(r.truncated);
        assert_eq!(r.summary, "äöü中文…");
    }

    #[test]
    fn blank_prompt_is_rejected_before_work() {
        let mut req = SubTaskRequest {
            prompt: "  \n\t ".to_string(),
            depth: 0,
            allowed_tools: None,
            limits: SubTaskLimits::default(),
            max_report_chars: 100,
        };
        assert!(!req.prompt_is_usable());
        req.prompt = "找出 src 下的所有 TODO".to_string();
        assert!(req.prompt_is_usable());
    }

    #[test]
    fn report_serializes_with_bounded_fields() {
        let paths: Vec<String> = (0..25).map(|i| format!("p{i}")).collect();
        let r = SubTaskReport::bounded(SubTaskStatus::BudgetExhausted, "x".repeat(50), paths, stats(), 8)
            .with_error_code("subtask_budget_exhausted");
        let v = serde_json::to_value(&r).expect("serialize");
        assert_eq!(v["status"], "budget_exhausted");
        assert_eq!(v["truncated"], true);
        assert_eq!(v["error_code"], "subtask_budget_exhausted");
        assert_eq!(v["stats"]["llm_calls"], 3);
        assert_eq!(v["key_paths"].as_array().map(Vec::len), Some(SubTaskReport::MAX_KEY_PATHS));
    }
}
