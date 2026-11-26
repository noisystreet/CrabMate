//! 完成判定**第 ② 层契约**：LLM 语义判定（#1387 S0.5）。
//!
//! 第 ① 层（[`crate::agent::agent_turn::policy::task_level_evidence::EvidenceSet`]）只回答
//! 「发生了什么」；本层回答「这些事实够不够算完成」。
//!
//! 设计见 `docs/design/completion_judge.md`。三条通用性约束落在本模块：
//!
//! 1. **后端无关**：输入是 `CompletionJudgeInput`（结构化 DTO），**不是 `messages`**——
//!    后者是后端内部表示（部分网关会把 `system` 合入 `user`），跨后端不稳定。
//! 2. **可确定性测试**：`CompletionJudge` 是 trait，单测与金样用 mock，不碰真模型。
//! 3. **判定与处置分离**：本模块只产出 `CompletionVerdict`，**不决定停不停**；
//!    「继续 / 停止」由编排层按剩余预算决定。
//!
//! **本模块不含任何 LLM 调用**：纯契约 + 机器可校验的锚点约束，因此可完全确定性测试。

use async_trait::async_trait;

use crate::cm_tool_result::ToolError;

/// 判定结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionStatus {
    /// 证据足以认定完成。
    Satisfied,
    /// 部分完成：`missing` 列出还差什么。
    Partial,
    /// 未完成，或锚点校验未通过。
    Unsatisfied,
}

impl CompletionStatus {
    /// 稳定短名（日志 / SSE / 金样；改名即契约变更）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Satisfied => "satisfied",
            Self::Partial => "partial",
            Self::Unsatisfied => "unsatisfied",
        }
    }

    /// 从模型 JSON 的 `status` 字段解析；未知值返回 `None`（由调用方 fail-open）。
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "satisfied" => Some(Self::Satisfied),
            "partial" => Some(Self::Partial),
            "unsatisfied" => Some(Self::Unsatisfied),
            _ => None,
        }
    }
}

/// 喂给判定器的单条证据（第 ① 层的投影）。
///
/// 字段刻意用 `String` 而非枚举：这是**跨后端 / 跨进程稳定**的 DTO，
/// 渲染与解析都在本仓一侧，不依赖 `messages` 形状。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JudgedEvidence {
    /// 稳定 id（`ev_1`…），供 `evidence_refs` 回引。
    pub id: String,
    /// `verify` / `write` / `other`。
    pub kind: String,
    /// `strong` / `weak`。
    pub strength: String,
    pub tool: String,
    pub ok: bool,
    pub exit_code: Option<i32>,
    /// 已截断的输出片段。
    pub detail: String,
}

impl JudgedEvidence {
    /// 是否为「能支撑完成结论」的硬证据种类。
    ///
    /// 只认 `verify` / `write`：`other`（只读探查、网络、调度）**不足以**单独支撑
    /// 「任务完成」——这条是锚点校验的第二道闸，用来挡住「引了一堆只读探查就宣称完成」。
    #[must_use]
    pub fn is_hard_evidence(&self) -> bool {
        matches!(self.kind.as_str(), "verify" | "write") && self.ok
    }
}

/// 判定输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionJudgeInput {
    /// 用户任务原文（最新一条真实 user 消息）。
    pub task: String,
    pub evidence: Vec<JudgedEvidence>,
}

/// 判定输出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionVerdict {
    pub status: CompletionStatus,
    /// 支撑结论的证据 id；`satisfied` 时**必须**非空且真实存在。
    pub evidence_refs: Vec<String>,
    /// 还差什么（`partial` / 降级原因）。
    pub missing: Vec<String>,
    /// 是否由**锚点校验**降级而来：模型原本给 `satisfied`，但 `evidence_refs` 引用了
    /// 不存在 / 非硬证据的 id，被 [`Self::validate_against`] 降级为 `unsatisfied`。
    ///
    /// 与「模型自己判 `partial` / `unsatisfied`」的语义**不同**：前者只是引用格式不合规，
    /// 并不代表任务没做完。终态层据此避免把「已完成但引用不规范」误报成未完成（#1472）。
    pub degraded: bool,
}

impl CompletionVerdict {
    /// `evidence_refs` 条数上限。
    pub const MAX_EVIDENCE_REFS: usize = 16;
    /// `missing` 条数与单条长度上限。
    pub const MAX_MISSING: usize = 8;
    pub const MAX_MISSING_CHARS: usize = 120;

    /// 唯一构造入口：施加条数与长度上限（模型输出不可信）。
    #[must_use]
    pub fn new(status: CompletionStatus, evidence_refs: Vec<String>, missing: Vec<String>) -> Self {
        let mut refs = evidence_refs;
        refs.truncate(Self::MAX_EVIDENCE_REFS);
        let mut missing: Vec<String> = missing
            .into_iter()
            .take(Self::MAX_MISSING)
            .map(|m| truncate_chars(m.trim(), Self::MAX_MISSING_CHARS))
            .filter(|m| !m.is_empty())
            .collect();
        missing.shrink_to_fit();
        Self {
            status,
            evidence_refs: refs,
            missing,
            degraded: false,
        }
    }

    /// 降级为 `unsatisfied` 并记录原因。
    #[must_use]
    pub fn degraded(mut self, reason: impl Into<String>) -> Self {
        self.status = CompletionStatus::Unsatisfied;
        self.degraded = true;
        let reason = reason.into();
        if !reason.trim().is_empty() && self.missing.len() < Self::MAX_MISSING {
            self.missing
                .push(truncate_chars(reason.trim(), Self::MAX_MISSING_CHARS));
        }
        self
    }

    /// **锚点校验**（设计 §4）：`satisfied` 必须同时满足
    ///
    /// 1. `evidence_refs` 非空，且每个 id 都在本次证据集合中；
    /// 2. 引用中至少一条是 `verify` / `write` 的**成功**证据。
    ///
    /// 任一不满足即降级为 `unsatisfied`。这里用「证据种类约束」代理了不可判定的
    /// 「语义是否真的支持结论」——零成本、可确定性测试。后者属评测问题，不进运行时。
    #[must_use]
    pub fn validate_against(self, evidence: &[JudgedEvidence]) -> Self {
        if self.status != CompletionStatus::Satisfied {
            return self;
        }
        if self.evidence_refs.is_empty() {
            return self.degraded("anchor: satisfied 但未给出 evidence_refs");
        }
        let mut cited_hard = false;
        let mut bad_ref: Option<String> = None;
        for want in &self.evidence_refs {
            match evidence.iter().find(|e| &e.id == want) {
                Some(found) => {
                    if found.is_hard_evidence() {
                        cited_hard = true;
                    }
                }
                None => {
                    bad_ref = Some(want.clone());
                    break;
                }
            }
        }
        if let Some(want) = bad_ref {
            return self.degraded(format!("anchor: evidence_refs 引用了不存在的证据 {want}"));
        }
        if !cited_hard {
            return self.degraded("anchor: 引用中没有 verify / write 类成功证据");
        }
        self
    }
}

/// 按 Unicode scalar 截断（不是字节）。
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if max_chars == 0 || s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push('…');
    out
}

/// 跑一次判定并**统一施加锚点校验**。
///
/// 返回 `None` 表示「判定不可用」（LLM 失败 / 超时 / 回复无法解析）——调用方据此
/// 维持第 ① 层结论，**绝不因此早停**（fail-open，见设计 §6）。
///
/// 把校验放在这里而不是各实现内部，是为了让 mock 实现也无法绕过
/// 「`satisfied` 必须引用硬证据」。
pub async fn judge_and_validate(
    judge: &dyn CompletionJudge,
    input: CompletionJudgeInput,
) -> Option<CompletionVerdict> {
    let evidence = input.evidence.clone();
    match judge.judge(input).await {
        Ok(verdict) => Some(verdict.validate_against(&evidence)),
        Err(e) => {
            log::debug!(
                target: "crabmate::completion_judge",
                "completion_judge unavailable code={} (fail-open, keep tier-1 verdict)",
                e.code
            );
            None
        }
    }
}

/// 完成判定器：生产实现调 LLM，测试用 mock。
///
/// 之所以是 trait 而非直接函数：LLM 判定天生不确定。若嵌进判定路径，金样会随模型波动，
/// CI 不可靠、后续无人敢改这块。trait 化后单测与金样用 mock 固定输出，真模型只进集成测试。
#[async_trait]
pub trait CompletionJudge: Send + Sync {
    /// 判定「给定证据是否足以认定任务完成」。
    ///
    /// 调用方必须 **fail-open**：本方法返回 `Err` 时维持第 ① 层结论，绝不因此早停。
    #[allow(clippy::result_large_err)] // `ToolError` 含正文解析快照，与其它宿主一致
    async fn judge(&self, input: CompletionJudgeInput) -> Result<CompletionVerdict, ToolError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: &str, kind: &str, ok: bool) -> JudgedEvidence {
        JudgedEvidence {
            id: id.to_string(),
            kind: kind.to_string(),
            strength: "strong".to_string(),
            tool: "cargo_test".to_string(),
            ok,
            exit_code: Some(0),
            detail: "test result: ok".to_string(),
        }
    }

    fn satisfied(refs: &[&str]) -> CompletionVerdict {
        CompletionVerdict::new(
            CompletionStatus::Satisfied,
            refs.iter().map(|s| s.to_string()).collect(),
            Vec::new(),
        )
    }

    #[test]
    fn status_strings_are_stable() {
        assert_eq!(CompletionStatus::Satisfied.as_str(), "satisfied");
        assert_eq!(CompletionStatus::Partial.as_str(), "partial");
        assert_eq!(CompletionStatus::Unsatisfied.as_str(), "unsatisfied");
        assert_eq!(
            CompletionStatus::parse("SATISFIED"),
            Some(CompletionStatus::Satisfied)
        );
        assert_eq!(CompletionStatus::parse("nope"), None);
    }

    #[test]
    fn satisfied_with_hard_evidence_passes() {
        let evidence = vec![ev("ev_1", "verify", true)];
        let v = satisfied(&["ev_1"]).validate_against(&evidence);
        assert_eq!(v.status, CompletionStatus::Satisfied);
        assert!(v.missing.is_empty());
    }

    #[test]
    fn satisfied_with_empty_refs_is_degraded() {
        // 模型说完成但一个证据都不指 → 无锚点，必须降级。
        let evidence = vec![ev("ev_1", "verify", true)];
        let v = satisfied(&[]).validate_against(&evidence);
        assert_eq!(v.status, CompletionStatus::Unsatisfied);
        assert!(v.missing.iter().any(|m| m.contains("evidence_refs")));
    }

    #[test]
    fn satisfied_with_unknown_ref_is_degraded() {
        let evidence = vec![ev("ev_1", "verify", true)];
        let v = satisfied(&["ev_99"]).validate_against(&evidence);
        assert_eq!(v.status, CompletionStatus::Unsatisfied);
        assert!(v.missing.iter().any(|m| m.contains("ev_99")));
    }

    #[test]
    fn satisfied_citing_only_other_is_degraded() {
        // 引了一堆只读探查就宣称完成——这是最需要挡的假阳性形态。
        let evidence = vec![ev("ev_1", "other", true), ev("ev_2", "other", true)];
        let v = satisfied(&["ev_1", "ev_2"]).validate_against(&evidence);
        assert_eq!(v.status, CompletionStatus::Unsatisfied);
        assert!(v.missing.iter().any(|m| m.contains("verify / write")));
    }

    #[test]
    fn satisfied_citing_failed_hard_evidence_is_degraded() {
        // 失败的验证不算硬证据。
        let evidence = vec![ev("ev_1", "verify", false)];
        let v = satisfied(&["ev_1"]).validate_against(&evidence);
        assert_eq!(v.status, CompletionStatus::Unsatisfied);
    }

    #[test]
    fn partial_is_not_subject_to_anchor_rules() {
        // partial / unsatisfied 无需锚点，照原样通过。
        let evidence = vec![ev("ev_1", "verify", true)];
        let v = CompletionVerdict::new(
            CompletionStatus::Partial,
            Vec::new(),
            vec!["未验证边界条件".to_string()],
        )
        .validate_against(&evidence);
        assert_eq!(v.status, CompletionStatus::Partial);
        assert_eq!(v.missing, vec!["未验证边界条件".to_string()]);
    }

    #[test]
    fn verdict_bounds_are_enforced() {
        let refs: Vec<String> = (0..40).map(|i| format!("ev_{i}")).collect();
        let missing: Vec<String> = (0..40).map(|i| format!("m{i}")).collect();
        let v = CompletionVerdict::new(CompletionStatus::Partial, refs, missing);
        assert_eq!(v.evidence_refs.len(), CompletionVerdict::MAX_EVIDENCE_REFS);
        assert_eq!(v.missing.len(), CompletionVerdict::MAX_MISSING);
    }

    #[test]
    fn missing_is_trimmed_and_empty_dropped() {
        let v = CompletionVerdict::new(
            CompletionStatus::Partial,
            Vec::new(),
            vec!["  有内容  ".to_string(), "   ".to_string()],
        );
        assert_eq!(v.missing, vec!["有内容".to_string()]);
    }

    #[test]
    fn long_missing_is_truncated_unicode_safe() {
        let long = "问".repeat(300);
        let v = CompletionVerdict::new(CompletionStatus::Partial, Vec::new(), vec![long]);
        assert_eq!(
            v.missing[0].chars().count(),
            CompletionVerdict::MAX_MISSING_CHARS + 1
        );
        assert!(v.missing[0].ends_with('…'));
    }

    /// 固定输出的 mock 判定器：用于验证 `judge_and_validate` 的组合行为
    /// （锚点统一施加 + fail-open），不碰真模型。
    struct FixedJudge(Result<CompletionVerdict, ()>);

    #[async_trait]
    impl CompletionJudge for FixedJudge {
        async fn judge(
            &self,
            _input: CompletionJudgeInput,
        ) -> Result<CompletionVerdict, ToolError> {
            match &self.0 {
                Ok(v) => Ok(v.clone()),
                Err(()) => Err(ToolError {
                    category: crate::cm_tool_result::ToolFailureCategory::Internal,
                    code: "mock_unavailable".to_string(),
                    message: "mock 判定不可用".to_string(),
                    retryable: false,
                    parsed: crate::cm_tool_result::ParsedToolOutput {
                        ok: false,
                        exit_code: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        error_code: None,
                    },
                }),
            }
        }
    }

    fn judge_input() -> CompletionJudgeInput {
        CompletionJudgeInput {
            task: "跑通全部测试".to_string(),
            evidence: vec![JudgedEvidence {
                id: "ev_1".to_string(),
                kind: "verify".to_string(),
                strength: "strong".to_string(),
                tool: "cargo_test".to_string(),
                ok: true,
                exit_code: Some(0),
                detail: "test result: ok".to_string(),
            }],
        }
    }

    /// 设计 §9 验收①：mock 说完成并引用了硬证据 → 原样通过。
    #[tokio::test]
    async fn judge_and_validate_passes_anchored_satisfied() {
        let judge = FixedJudge(Ok(satisfied(&["ev_1"])));
        let out = judge_and_validate(&judge, judge_input())
            .await
            .expect("some");
        assert_eq!(out.status, CompletionStatus::Satisfied);
    }

    /// 设计 §9 验收②：mock 说完成但 `evidence_refs` 为空 → **降级**。
    ///
    /// 这条是整条链路最关键的护栏：即使判定器（含未来任何实现）给出 satisfied，
    /// 只要拿不出锚点，也不得据此早停。
    #[tokio::test]
    async fn judge_and_validate_degrades_unanchored_satisfied() {
        let judge = FixedJudge(Ok(satisfied(&[])));
        let out = judge_and_validate(&judge, judge_input())
            .await
            .expect("some");
        assert_eq!(out.status, CompletionStatus::Unsatisfied);
        assert!(out.missing.iter().any(|m| m.contains("evidence_refs")));
    }

    /// 设计 §9 验收③：判定器失败 → 返回 `None`（fail-open）。
    ///
    /// 调用方拿到 `None` 时必须维持第 ① 层结论；**绝不能**据此早停。
    #[tokio::test]
    async fn judge_and_validate_returns_none_on_error() {
        let judge = FixedJudge(Err(()));
        assert!(judge_and_validate(&judge, judge_input()).await.is_none());
    }
}
