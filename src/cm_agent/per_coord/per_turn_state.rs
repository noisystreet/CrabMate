//! 单轮 `run_agent_turn` 内与 PER 协调相关的**可变回合状态**，从 [`super::PerCoordinator`] 顶层字段拆出，
//! 便于一眼区分：**配置镜像 / 策略来源** vs **本回合计数** vs **派生缓存** vs **工具失败短路表**。
//!
//! 归属总表见 **`docs/design/run_loop_state_ownership.md`**。
//!
//! - **[`PerTurnCounters`]**：终答 `plan_rewrite` 已用次数 + 暂住的 **[`OuterLoopReflectMemo`]**（R 轨 3）。
//! - **[`WorkflowValidateLayerCache`]**：`last_workflow_validate_layer_count` 随 `messages.len()` 的缓存；上下文裁剪后必须失效。
//! - **[`RepeatedToolFailureMemo`]**：同轮工具失败签名 / 族短路（只读查询 + 记录清除）。
//! - **[`SuccessfulRunCommandDedupeMemo`]**：同轮已成功构建/运行命令的结果缓存（防重复 spawn）。

use crate::cm_types::Message;
use std::collections::HashMap;

use crate::cm_agent::plan_rewrite;
use crate::cm_agent::completion_judge::CompletionVerdict;

/// 外循环 Gate **前**纠偏计数（R 轨 3：`OuterLoopReflectPreGateReason`）。
///
/// **语义归属**：外循环 / `outer_loop_reflect`，**不是**终答 Gate 或工作流反思 FSM。
/// **物理位置**：暂住 [`PerTurnCounters`]（经 `PerCoordinator` 委托），便于单轮共享；勿与
/// `plan_rewrite_attempts` 混读。见 **`docs/design/run_loop_state_ownership.md`**。
#[derive(Debug, Clone, Default)]
pub(crate) struct OuterLoopReflectMemo {
    pub(crate) missing_final_answer_feedback_injected: u32,
    pub(crate) truncated_feedback_injected: u32,
}

impl OuterLoopReflectMemo {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn record_missing_final_answer_feedback_injected(&mut self) {
        self.missing_final_answer_feedback_injected = self
            .missing_final_answer_feedback_injected
            .saturating_add(1);
    }

    pub(crate) fn missing_final_answer_feedback_injected(&self) -> u32 {
        self.missing_final_answer_feedback_injected
    }

    pub(crate) fn record_truncated_feedback_injected(&mut self) {
        self.truncated_feedback_injected = self.truncated_feedback_injected.saturating_add(1);
    }

    pub(crate) fn truncated_feedback_injected(&self) -> u32 {
        self.truncated_feedback_injected
    }
}

/// 本 `run_agent_turn` 内、与配置上限对照的**正交**计数器。
///
/// - **`plan_rewrite_attempts`**：终答路径 `agent_reply_plan` 不合格时追加重写 user 的已用次数（与 **`plan_rewrite_max_attempts`** 对照）。
/// - **`outer_loop_reflect`**：外循环 pre-gate 纠偏（见 [`OuterLoopReflectMemo`]）。
#[derive(Debug, Clone)]
pub(crate) struct PerTurnCounters {
    pub(crate) plan_rewrite_attempts: usize,
    pub(crate) outer_loop_reflect: OuterLoopReflectMemo,
    /// 上次询问完成判定第 ② 层（LLM）时的**证据指纹**；`None` = 本回合尚未问过。
    ///
    /// 存在的理由：早停门在**每个工具批之后**都会走一次（`outer_loop` 的外循环内），
    /// 而第 ① 层对综合任务恒为 `NotApplicable`。若不做去重，一个 50 轮的长任务会发
    /// 50 次判定请求。用「证据是否变化」而不是「是否问过」来判断：有新证据才值得再问，
    /// 同一份事实重复问没有意义。
    pub(crate) completion_judge_evidence_fingerprint: Option<u64>,
    /// 本回合已询问完成判定第 ② 层的次数。
    ///
    /// 光靠指纹去重不够：**每轮新工具调用都会改变指纹**，实测 5 轮工具批 = 5 次调用。
    /// 早停门的价值在于省下**后续**轮次，而任务刚开始时即使判出完成也只省一两轮——
    /// 收益远小于一次 LLM 往返。因此加每回合硬上限，默认
    /// [`MAX_COMPLETION_JUDGE_CALLS_PER_TURN`]。
    pub(crate) completion_judge_calls: u32,
    /// 本回合是否已确认「活跃用户目标完成」（#1415）。
    ///
    /// **为什么需要缓存**：完成判定的两个消费者不在同一位置、也不都是 async——
    /// 早停门 `turn_early_stop_allowed_with_judge` 在**工具执行之后**、且能问 LLM；
    /// 而冗余探针抑制 `try_stop_on_completed_goal_redundant_tools` 在**下一次模型给出
    /// `tool_calls` 之后**、且是同步纯逻辑。同一批证据如果各判一次，就会出现
    /// 「早停门放行、抑制门不放行」的分裂（#1415 删掉关键词门后尤其明显）。
    /// 结论由判定方写入，同步方只读，两边口径因此天然一致。
    pub(crate) completion_confirmed: bool,
    /// 本回合完成判定第 ② 层的**结构化结论**（#1472）。
    ///
    /// 与 `completion_confirmed` 的区别：后者只回答「是否确认完成」，本字段保留
    /// `Partial` 与 `missing`（还差什么），供收尾层区分「正常完成」与「有终答但未完成」。
    /// 不参与早停门的 stop / continue 决策。
    pub(crate) completion_verdict: Option<CompletionVerdict>,
}

/// 每回合最多询问完成判定第 ② 层的次数。
///
/// 取 3 的理由：早停若真有价值，通常在前几轮就能判出来；再多问已属边际收益递减。
/// 需要更大额度时再升为配置键（届时同步 `docs/配置说明.md`）。
pub(crate) const MAX_COMPLETION_JUDGE_CALLS_PER_TURN: u32 = 3;

impl PerTurnCounters {
    pub(crate) fn new() -> Self {
        Self {
            plan_rewrite_attempts: 0,
            outer_loop_reflect: OuterLoopReflectMemo::new(),
            completion_judge_evidence_fingerprint: None,
            completion_judge_calls: 0,
            completion_confirmed: false,
            completion_verdict: None,
        }
    }

    /// 记录「活跃用户目标已完成」。只由判定方（早停门）写入；同步消费者只读，
    /// 避免同一批证据被两道门各判一次而给出相反结论。
    pub(crate) fn mark_completion_confirmed(&mut self) {
        self.completion_confirmed = true;
    }

    /// 本回合是否已确认完成（供同步门读取判定方写入的结论）。
    pub(crate) fn completion_confirmed(&self) -> bool {
        self.completion_confirmed
    }

    /// 本次该不该问第 ② 层。
    ///
    /// 两道闸：① 每回合总次数不超过 [`MAX_COMPLETION_JUDGE_CALLS_PER_TURN`]（成本上限）；
    /// ② 证据指纹与上次相同则跳过（同一份事实重复问没意义）。任一不满足即不调用 LLM。
    pub(crate) fn take_completion_judge_slot(&mut self, fingerprint: u64) -> bool {
        // 证据没变 → 上次结论仍然成立，也不必再问（去重）。
        if self.completion_judge_evidence_fingerprint == Some(fingerprint) {
            return false;
        }
        // 证据已变但本回合判定预算耗尽 → 之后不会再判，旧结论**可能已过期**
        // （典型形态：判过 `unsatisfied` 后模型又干活并成功）。#1472 要求这种情形按
        // fail-open 回到 `completed`，绝不能用过期结论把已完成的回合报成 `partial`。
        if self.completion_judge_calls >= MAX_COMPLETION_JUDGE_CALLS_PER_TURN {
            self.completion_verdict = None;
            return false;
        }
        self.completion_judge_evidence_fingerprint = Some(fingerprint);
        self.completion_judge_calls = self.completion_judge_calls.saturating_add(1);
        true
    }

    pub(crate) fn record_outer_loop_missing_final_answer_feedback_injected(&mut self) {
        self.outer_loop_reflect
            .record_missing_final_answer_feedback_injected();
    }

    pub(crate) fn outer_loop_missing_final_answer_feedback_injected(&self) -> u32 {
        self.outer_loop_reflect
            .missing_final_answer_feedback_injected()
    }

    pub(crate) fn record_outer_loop_truncated_feedback_injected(&mut self) {
        self.outer_loop_reflect.record_truncated_feedback_injected();
    }

    pub(crate) fn outer_loop_truncated_feedback_injected(&self) -> u32 {
        self.outer_loop_reflect.truncated_feedback_injected()
    }

    /// 记录第 ② 层判定结论（含 `partial` / `missing`，见 #1472）。
    ///
    /// 只由判定方（早停门）写入；收尾时经 [`Self::take_completion_verdict`] 搬去
    /// `PerTurnFlight`，不与 `completion_confirmed` 互相影响。
    pub(crate) fn record_completion_verdict(&mut self, verdict: CompletionVerdict) {
        self.completion_verdict = Some(verdict);
    }

    /// 取出第 ② 层判定结论（收尾时搬去 `PerTurnFlight`；取出后本回合不再持有）。
    pub(crate) fn take_completion_verdict(&mut self) -> Option<CompletionVerdict> {
        self.completion_verdict.take()
    }
}

/// 缓存 [`plan_rewrite::last_workflow_validate_layer_count`]：仅在 `messages.len()` 与上次一致且已有缓存时跳过全表扫描。
#[derive(Debug, Clone)]
pub(crate) struct WorkflowValidateLayerCache {
    cached_workflow_validate_layer_count: Option<usize>,
    layer_count_cache_at_message_len: usize,
}

impl WorkflowValidateLayerCache {
    pub(crate) fn new() -> Self {
        Self {
            cached_workflow_validate_layer_count: None,
            layer_count_cache_at_message_len: 0,
        }
    }

    pub(crate) fn invalidate_after_context_mutation(&mut self) {
        self.cached_workflow_validate_layer_count = None;
        self.layer_count_cache_at_message_len = 0;
    }

    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> (Option<usize>, usize) {
        (
            self.cached_workflow_validate_layer_count,
            self.layer_count_cache_at_message_len,
        )
    }

    pub(crate) fn workflow_validate_layer_need(&mut self, messages: &[Message]) -> Option<usize> {
        let len = messages.len();
        if len != self.layer_count_cache_at_message_len {
            let n = plan_rewrite::last_workflow_validate_layer_count(messages);
            self.cached_workflow_validate_layer_count = n;
            self.layer_count_cache_at_message_len = len;
            return n;
        }
        if self.cached_workflow_validate_layer_count.is_some() {
            return self.cached_workflow_validate_layer_count;
        }
        let n = plan_rewrite::last_workflow_validate_layer_count(messages);
        self.cached_workflow_validate_layer_count = n;
        self.layer_count_cache_at_message_len = len;
        n
    }

    /// `append_tool_result_and_reflection` 在追加 tool（及可选 user）后同步缓存与扫描结果。
    pub(crate) fn refresh_after_messages_append(
        &mut self,
        messages_len: usize,
        messages: &[Message],
    ) {
        self.layer_count_cache_at_message_len = messages_len;
        self.cached_workflow_validate_layer_count =
            plan_rewrite::last_workflow_validate_layer_count(messages);
    }
}

/// 同一回合内工具失败记忆：精确签名与「错误族」两级短路。
#[derive(Debug, Clone)]
pub(crate) struct RepeatedToolFailureMemo {
    repeated_failed_tool_signatures: HashMap<(String, String), String>,
    repeated_failed_tool_families: HashMap<(String, String), String>,
}

impl RepeatedToolFailureMemo {
    pub(crate) fn new() -> Self {
        Self {
            repeated_failed_tool_signatures: HashMap::new(),
            repeated_failed_tool_families: HashMap::new(),
        }
    }

    pub(crate) fn repeated_tool_failure_error_marker(
        &self,
        tool_name: &str,
        tool_args_json: &str,
    ) -> Option<&str> {
        self.repeated_failed_tool_signatures
            .get(&(tool_name.to_string(), tool_args_json.to_string()))
            .map(|s| s.as_str())
    }

    pub(crate) fn mark_tool_failure_signature(
        &mut self,
        tool_name: &str,
        tool_args_json: &str,
        error_marker: String,
    ) {
        self.repeated_failed_tool_signatures.insert(
            (tool_name.to_string(), tool_args_json.to_string()),
            error_marker,
        );
    }

    pub(crate) fn repeated_tool_failure_family_marker(
        &self,
        tool_name: &str,
        failure_family: &str,
    ) -> Option<&str> {
        self.repeated_failed_tool_families
            .get(&(tool_name.to_string(), failure_family.to_string()))
            .map(|s| s.as_str())
    }

    pub(crate) fn mark_tool_failure_family(
        &mut self,
        tool_name: &str,
        failure_family: &str,
        error_marker: String,
    ) {
        self.repeated_failed_tool_families.insert(
            (tool_name.to_string(), failure_family.to_string()),
            error_marker,
        );
    }

    pub(crate) fn clear_tool_failure_signature(&mut self, tool_name: &str, tool_args_json: &str) {
        self.repeated_failed_tool_signatures
            .remove(&(tool_name.to_string(), tool_args_json.to_string()));
    }

    pub(crate) fn clear_tool_failure_families_for_tool(&mut self, tool_name: &str) {
        self.repeated_failed_tool_families
            .retain(|(name, _), _| name != tool_name);
    }

    pub(crate) fn clear_all_tool_failure_state_for_tool(&mut self, tool_name: &str) {
        self.repeated_failed_tool_signatures
            .retain(|(name, _), _| name != tool_name);
        self.clear_tool_failure_families_for_tool(tool_name);
    }
}

/// 同一回合内已成功执行过的构建/运行类 `run_command` 缓存。
#[derive(Debug, Clone, Default)]
pub(crate) struct SuccessfulRunCommandDedupeMemo {
    outputs: HashMap<String, String>,
}

impl SuccessfulRunCommandDedupeMemo {
    pub(crate) fn new() -> Self {
        Self {
            outputs: HashMap::new(),
        }
    }

    pub(crate) fn cached_output(&self, suppress_key: &str) -> Option<&str> {
        self.outputs.get(suppress_key).map(|s| s.as_str())
    }

    pub(crate) fn record_success(&mut self, suppress_key: String, output: String) {
        self.outputs.insert(suppress_key, output);
    }

    pub(crate) fn clear_all(&mut self) {
        self.outputs.clear();
    }
}

#[cfg(test)]
mod completion_judge_slot_tests {
    use super::PerTurnCounters;

    /// **同一份证据不得重复放行**：早停门在每个工具批之后都会走一次，
    /// 不去重则长任务每轮发一次判定请求。
    #[test]
    fn slot_is_taken_once_per_fingerprint() {
        let mut c = PerTurnCounters::new();
        let fp = 42u64;
        assert!(c.take_completion_judge_slot(fp), "首次应放行");
        assert!(!c.take_completion_judge_slot(fp), "同一指纹第二次必须跳过");
        assert!(!c.take_completion_judge_slot(fp), "仍然跳过");
        assert!(
            c.take_completion_judge_slot(fp + 1),
            "证据变化后应重新放行"
        );
    }

    /// 新回合从「未问过」开始。
    #[test]
    fn fresh_counters_allow_first_slot() {
        let mut c = PerTurnCounters::new();
        assert!(c.take_completion_judge_slot(0));
    }

    /// **每回合硬上限**：即使每轮都有新证据，调用次数也不得超过上限。
    ///
    /// 这条是成本护栏——实测 5 轮工具批会产生 5 个不同指纹，
    /// 只靠指纹去重挡不住，必须叠加次数上限。
    #[test]
    fn slot_is_capped_per_turn_even_with_changing_evidence() {
        let mut c = PerTurnCounters::new();
        let mut allowed = 0u32;
        for i in 0..50u64 {
            if c.take_completion_judge_slot(i) {
                allowed += 1;
            }
        }
        assert_eq!(
            allowed,
            super::MAX_COMPLETION_JUDGE_CALLS_PER_TURN,
            "证据持续变化时也必须停在每回合上限"
        );
    }

    /// #1472：预算耗尽后旧结论必须失效——否则「判过 `unsatisfied`、之后又干活干完」的回合
    /// 会被过期结论误报成 `partial`。因此「证据变了」+「预算尽」时必须清结论（fail-open）。
    #[test]
    fn exhausted_budget_invalidates_stale_verdict() {
        use crate::cm_agent::completion_judge::{CompletionStatus, CompletionVerdict};
        let mut c = PerTurnCounters::new();
        let unsat = || CompletionVerdict::new(CompletionStatus::Unsatisfied, Vec::new(), Vec::new());
        for fp in 0..super::MAX_COMPLETION_JUDGE_CALLS_PER_TURN as u64 {
            if c.take_completion_judge_slot(fp) {
                c.record_completion_verdict(unsat());
            }
        }
        // 预算已尽，又来新证据 → 不再判定，且旧结论必须失效
        assert!(!c.take_completion_judge_slot(999));
        assert!(
            c.take_completion_verdict().is_none(),
            "预算耗尽后过期结论必须失效（fail-open 回到 completed）"
        );
    }

    /// #1472：证据**未变**时去重，但已有结论仍然成立，不得被清除。
    #[test]
    fn same_fingerprint_keeps_verdict() {
        use crate::cm_agent::completion_judge::{CompletionStatus, CompletionVerdict};
        let mut c = PerTurnCounters::new();
        assert!(c.take_completion_judge_slot(7));
        c.record_completion_verdict(CompletionVerdict::new(
            CompletionStatus::Partial,
            Vec::new(),
            Vec::new(),
        ));
        assert!(!c.take_completion_judge_slot(7), "同一指纹应跳过");
        assert!(
            c.take_completion_verdict().is_some(),
            "证据没变时结论仍然成立，不得被清除"
        );
    }
}
