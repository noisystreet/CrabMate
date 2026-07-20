//! 从 `messages` 扫描工作流工具结果、组装规划重写 user 正文、终答侧向校验摘要、用尽原因分类。
//! **不**调用 `complete_chat_retrying`（侧向 LLM 仍在 [`crate::agent::per_plan_semantic_check`]）。

use crabmate_tools::tool_result::{
    normalize_tool_message_content, tool_message_payload_for_inner_parse,
};
use crabmate_types::Message;
use serde_json::Value;

use crate::log_preview::preview_chars;
use crate::plan_artifact;

/// 终答规划重写用尽时 SSE **`reason_code`** 的稳定子码（顶层 **`code`** 仍为 `plan_rewrite_exhausted`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanRewriteExhaustedReason {
    /// 正文无可解析的 `agent_reply_plan` v1。
    PlanMissing,
    /// `steps` 条数低于最近一次 `workflow_validate` 的 `layer_count` 要求。
    PlanLayerCountMismatch,
    /// `workflow_node_id` 与最近工作流节点 id 集合不一致（子集校验失败）。
    PlanWorkflowNodeIdsInvalid,
    /// 严格模式下未覆盖全部工作流节点 id。
    PlanWorkflowNodeCoverageIncomplete,
    /// `workflow_validate_only` 后规划未与 **`nodes[].id` 一一绑定**（步数、`workflow_node_id` 必填或多重集合不一致）。
    PlanValidateOnlyNodeBindingMismatch,
    /// 侧向语义校验判定与工具结果矛盾且重写次数已用尽。
    PlanSemanticInconsistent,
    /// 未归类的用尽路径（防御性；主路径不应出现）。
    ExhaustedOther,
}

impl PlanRewriteExhaustedReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PlanMissing => "plan_missing",
            Self::PlanLayerCountMismatch => "plan_layer_count_mismatch",
            Self::PlanWorkflowNodeIdsInvalid => "plan_workflow_node_ids_invalid",
            Self::PlanWorkflowNodeCoverageIncomplete => "plan_workflow_node_coverage_incomplete",
            Self::PlanValidateOnlyNodeBindingMismatch => "plan_validate_only_node_binding_mismatch",
            Self::PlanSemanticInconsistent => "plan_semantic_inconsistent",
            Self::ExhaustedOther => "plan_rewrite_exhausted_other",
        }
    }
}

/// 侧向语义校验失败时追加的 user 正文：自然语言说明 + 机器可读 `crabmate_plan_semantic_feedback` + 规划重写要求。
pub fn user_text_semantic_mismatch_with_feedback(
    violation_codes: &[String],
    rationale: Option<&str>,
) -> String {
    let codes_json: Vec<&str> = if violation_codes.is_empty() {
        vec!["semantic_mismatch_unspecified"]
    } else {
        violation_codes.iter().map(String::as_str).collect()
    };
    let rationale_json = rationale
        .map(|s| Value::String(s.to_string()))
        .unwrap_or(Value::Null);
    let machine = serde_json::json!({
        "kind": "crabmate_plan_semantic_feedback",
        "version": 1,
        "violation_codes": codes_json,
        "rationale": rationale_json,
    });
    let machine_line = serde_json::to_string(&machine).unwrap_or_else(|_| {
        "{\"kind\":\"crabmate_plan_semantic_feedback\",\"version\":1,\"violation_codes\":[\"semantic_mismatch_unspecified\"],\"rationale\":null}".to_string()
    });
    format!(
        "侧向校验认为你的 **agent_reply_plan** 与**最近工具执行结果**存在明显矛盾。请根据下方 **violation_codes**（及可选 **rationale**）与**真实工具结果**修正规划 JSON 与说明文字。\n\n```json\n{}\n```\n\n{}",
        machine_line,
        plan_rewrite_user_text_base()
    )
}

pub fn plan_rewrite_user_text_base() -> String {
    format!(
        "你的最终回答缺少**结构化规划**。请在 content 中加入一段 Markdown 代码围栏（语言标记为 json），其内为合法 JSON，且必须满足：\n{}\n\n示例：\n```json\n{}\n```\n\n请直接重写本轮最终回答（可有其它说明文字，但须包含上述 JSON 围栏）。",
        plan_artifact::PLAN_V1_SCHEMA_RULES,
        plan_artifact::PLAN_V1_EXAMPLE_JSON
    )
}

fn exhausted_layer_count_mismatch(
    plan: &plan_artifact::AgentReplyPlanV1,
    layer_need: Option<usize>,
    apply_layer_semantics: bool,
) -> bool {
    matches!(
        layer_need,
        Some(n) if n > 0 && apply_layer_semantics && plan.steps.len() < n
    )
}

fn exhausted_workflow_subset_mismatch(
    plan: &plan_artifact::AgentReplyPlanV1,
    wf_ids: Option<&Vec<String>>,
) -> bool {
    wf_ids
        .map(|ids| plan_artifact::validate_plan_workflow_node_ids_subset(plan, ids).is_err())
        .unwrap_or(false)
}

fn exhausted_workflow_coverage_mismatch(
    plan: &plan_artifact::AgentReplyPlanV1,
    wf_ids: Option<&Vec<String>>,
    strict_workflow_node_coverage: bool,
) -> bool {
    strict_workflow_node_coverage
        && wf_ids
            .map(|ids| {
                plan_artifact::validate_plan_covers_all_workflow_node_ids(plan, ids).is_err()
            })
            .unwrap_or(false)
}

fn exhausted_validate_only_binding_mismatch(
    plan: &plan_artifact::AgentReplyPlanV1,
    messages: &[Message],
    apply_layer_semantics: bool,
) -> bool {
    apply_layer_semantics
        && last_workflow_validate_binding_plan_node_ids(messages)
            .filter(|ids| !ids.is_empty())
            .map(|ids| {
                plan_artifact::validate_plan_binds_workflow_validate_nodes(plan, ids.as_slice())
                    .is_err()
            })
            .unwrap_or(false)
}

pub fn classify_exhausted_reason(
    msg: &Message,
    messages: &[Message],
    layer_need: Option<usize>,
    apply_layer_semantics: bool,
    strict_workflow_node_coverage: bool,
) -> PlanRewriteExhaustedReason {
    let content = crabmate_types::message_content_as_str(&msg.content).unwrap_or("");
    let validate_only_binding_ids = if apply_layer_semantics {
        last_workflow_validate_binding_plan_node_ids(messages)
    } else {
        None
    };
    let Ok(plan) = plan_artifact::parse_agent_reply_plan_v1_with_validate_only_binding_ids(
        content,
        validate_only_binding_ids.as_deref(),
    ) else {
        return PlanRewriteExhaustedReason::PlanMissing;
    };
    if exhausted_layer_count_mismatch(&plan, layer_need, apply_layer_semantics) {
        return PlanRewriteExhaustedReason::PlanLayerCountMismatch;
    }
    let wf_ids = last_workflow_tool_node_ids(messages);
    if exhausted_workflow_subset_mismatch(&plan, wf_ids.as_ref()) {
        return PlanRewriteExhaustedReason::PlanWorkflowNodeIdsInvalid;
    }
    if exhausted_workflow_coverage_mismatch(&plan, wf_ids.as_ref(), strict_workflow_node_coverage) {
        return PlanRewriteExhaustedReason::PlanWorkflowNodeCoverageIncomplete;
    }
    if exhausted_validate_only_binding_mismatch(&plan, messages, apply_layer_semantics) {
        return PlanRewriteExhaustedReason::PlanValidateOnlyNodeBindingMismatch;
    }
    PlanRewriteExhaustedReason::ExhaustedOther
}

// 解析单条 `role: tool` 消息：若为 `workflow_execute` 且报告类型与 `nodes` 合法则返回 id 列表。
fn try_node_ids_from_workflow_execute_tool_message(
    messages: &[Message],
    tool_idx: usize,
) -> Option<Vec<String>> {
    let m = &messages[tool_idx];
    let tid = m.tool_call_id.as_deref()?;
    let aidx = assistant_index_for_tool_call(messages, tool_idx, tid)?;
    let assistant = &messages[aidx];
    let name = assistant
        .tool_calls
        .as_ref()?
        .iter()
        .find(|c| c.id == tid)
        .map(|c| c.function.name.as_str())?;
    if name != "workflow_execute" {
        return None;
    }
    let body = crabmate_types::message_content_as_str(&m.content)?;
    let payload = tool_message_payload_for_inner_parse(body);
    let v: Value = serde_json::from_str(payload.as_ref()).ok()?;
    let rt = v.get("report_type").and_then(|x| x.as_str());
    if !matches!(
        rt,
        Some("workflow_validate_result") | Some("workflow_execute_result")
    ) {
        return None;
    }
    let nodes = v.get("nodes").and_then(|x| x.as_array())?;
    let mut ids = Vec::new();
    for n in nodes {
        let id = n.get("id").and_then(|x| x.as_str())?;
        ids.push(id.to_string());
    }
    if ids.is_empty() {
        return None;
    }
    Some(ids)
}

/// 从对话历史中取**最近一次** `workflow_execute` 工具结果中的 `nodes[].id`（`workflow_validate_result` / `workflow_execute_result`）。
pub fn last_workflow_tool_node_ids(messages: &[Message]) -> Option<Vec<String>> {
    for i in (0..messages.len()).rev() {
        if messages[i].role != "tool" {
            continue;
        }
        if let Some(ids) = try_node_ids_from_workflow_execute_tool_message(messages, i) {
            return Some(ids);
        }
    }
    None
}

/// 从对话历史中取**最近一次** `workflow_execute` 的 `workflow_validate_only` 工具结果里的 `spec.layer_count`。
pub fn last_workflow_validate_layer_count(messages: &[Message]) -> Option<usize> {
    for i in (0..messages.len()).rev() {
        let m = &messages[i];
        if m.role != "tool" {
            continue;
        }
        let tid = m.tool_call_id.as_deref()?;
        let aidx = assistant_index_for_tool_call(messages, i, tid)?;
        let assistant = &messages[aidx];
        let name = assistant
            .tool_calls
            .as_ref()?
            .iter()
            .find(|c| c.id == tid)
            .map(|c| c.function.name.as_str())?;
        if name != "workflow_execute" {
            continue;
        }
        let body = crabmate_types::message_content_as_str(&m.content)?;
        let payload = tool_message_payload_for_inner_parse(body);
        let v: Value = serde_json::from_str(payload.as_ref()).ok()?;
        if v.get("report_type").and_then(|x| x.as_str()) != Some("workflow_validate_result") {
            continue;
        }
        let n = v
            .get("spec")
            .and_then(|s| s.get("layer_count"))
            .and_then(|x| x.as_u64())? as usize;
        return Some(n);
    }
    None
}

/// 自历史中取最近一次 **`workflow_validate_result`** 的 `nodes[].id` 列表（含重复），供 **validate_only → Do** 路径上强制规划逐步绑定。
pub fn last_workflow_validate_binding_plan_node_ids(messages: &[Message]) -> Option<Vec<String>> {
    for i in (0..messages.len()).rev() {
        let m = &messages[i];
        if m.role != "tool" {
            continue;
        }
        let Some(tid) = m.tool_call_id.as_deref() else {
            continue;
        };
        let Some(aidx) = assistant_index_for_tool_call(messages, i, tid) else {
            continue;
        };
        let assistant = &messages[aidx];
        let Some(name) = assistant
            .tool_calls
            .as_ref()
            .and_then(|tc| tc.iter().find(|c| c.id == tid))
            .map(|c| c.function.name.as_str())
        else {
            continue;
        };
        if name != "workflow_execute" {
            continue;
        }
        let Some(body) = crabmate_types::message_content_as_str(&m.content) else {
            continue;
        };
        let payload = tool_message_payload_for_inner_parse(body);
        let Ok(v) = serde_json::from_str::<Value>(payload.as_ref()) else {
            continue;
        };
        if v.get("report_type").and_then(|x| x.as_str()) != Some("workflow_validate_result") {
            continue;
        }
        let Some(nodes) = v.get("nodes").and_then(|x| x.as_array()) else {
            continue;
        };
        let mut ids = Vec::new();
        for n in nodes {
            let Some(id) = n.get("id").and_then(|x| x.as_str()) else {
                continue;
            };
            ids.push(id.to_string());
        }
        if ids.is_empty() {
            continue;
        }
        return Some(ids);
    }
    None
}

pub fn validate_only_plan_binding_rewrite_suffix(validate_only_node_ids: &[String]) -> String {
    if validate_only_node_ids.is_empty() {
        return String::new();
    }
    let n = validate_only_node_ids.len();
    format!(
        "\n\n**validate_only 绑定点（必守）**：最近一次 `workflow_validate_only` 的 `nodes` 共 **{n}** 个（DAG 顺序可异于下列列表，但绑定须一致）。你的 `agent_reply_plan` 须满足：\n\
1. `steps.len()` **等于** **{n}**（与 `nodes` 个数相同）。\n\
2. **每一步**均须设置 **`workflow_node_id`**（不得省略）。\n\
3. 全部 `workflow_node_id` 构成的**多重集合**须与下列节点 id **完全一致**（含重复次数；`steps` 顺序可与下列不同）：`{}`。",
        validate_only_node_ids.join(", ")
    )
}

/// 内置工具名：出现则认为「高风险」，语义侧向校验可收录其摘要（仍受 `max_non_readonly` 条数限制）。
const SEMANTIC_CHECK_HIGH_RISK_TOOLS: &[&str] = &[
    "run_command",
    "run_executable",
    "workflow_execute",
    "create_file",
    "edit_file",
    "apply_patch",
    "http_request",
];

fn tool_name_is_high_risk(name: &str) -> bool {
    SEMANTIC_CHECK_HIGH_RISK_TOOLS.contains(&name) || name.starts_with("mcp__")
}

fn semantic_check_tool_name_body_for_message(
    messages: &[Message],
    tool_idx: usize,
) -> Option<(&str, &str)> {
    let m = &messages[tool_idx];
    let tid = m.tool_call_id.as_deref()?;
    let aidx = assistant_index_for_tool_call(messages, tool_idx, tid)?;
    let assistant = &messages[aidx];
    let tc = assistant
        .tool_calls
        .as_ref()?
        .iter()
        .find(|c| c.id == tid)?;
    let name = tc.function.name.as_str();
    let body = crabmate_types::message_content_as_str(&m.content).unwrap_or("");
    Some((name, body))
}

/// 只读工具不计入 `non_ro_used`；非只读且已达上限且非高风险则返回 `false`（跳过该条）。
fn semantic_check_include_tool_line(
    is_readonly_tool: &dyn Fn(&str) -> bool,
    name: &str,
    non_ro_used: &mut usize,
    max_non_readonly_tools: usize,
) -> bool {
    let is_ro = is_readonly_tool(name);
    let is_risky = tool_name_is_high_risk(name);
    if is_ro {
        return true;
    }
    if *non_ro_used >= max_non_readonly_tools && !is_risky {
        return false;
    }
    *non_ro_used += 1;
    true
}

fn semantic_check_format_tool_summary_line(name: &str, body: &str, max_chars_per: usize) -> String {
    let mut line = if let Some(env) = normalize_tool_message_content(body) {
        let out_head = preview_chars(env.output.as_str(), 320);
        format!(
            "- {} ok={} summary={} out_preview={}",
            env.name,
            env.ok,
            crabmate_tools::redact::single_line_preview(env.summary.as_str(), 160),
            out_head
        )
    } else {
        format!(
            "- {} legacy_preview={}",
            name,
            preview_chars(body, max_chars_per)
        )
    };
    if line.chars().count() > max_chars_per {
        line = preview_chars(&line, max_chars_per);
    }
    line
}

/// 自尾向前收集最近若干条 `role: tool` 的短摘要，供终答规划侧向 LLM 使用；无工具则 `None`。
pub fn summarize_messages_for_final_plan_semantic_check(
    messages: &[Message],
    is_readonly_tool: &dyn Fn(&str) -> bool,
    workspace_is_set: bool,
    max_non_readonly_tools: usize,
) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut non_ro_used = 0usize;
    const MAX_LINES: usize = 12;
    const MAX_CHARS_PER: usize = 900;

    for i in (0..messages.len()).rev() {
        if lines.len() >= MAX_LINES {
            break;
        }
        if messages[i].role != "tool" {
            continue;
        }
        let (name, body) = semantic_check_tool_name_body_for_message(messages, i)?;
        if !semantic_check_include_tool_line(
            is_readonly_tool,
            name,
            &mut non_ro_used,
            max_non_readonly_tools,
        ) {
            continue;
        }
        lines.push(semantic_check_format_tool_summary_line(
            name,
            body,
            MAX_CHARS_PER,
        ));
    }

    if lines.is_empty() {
        return None;
    }
    lines.reverse();
    let header = if workspace_is_set {
        "以下为逆序收集的最近工具结果摘要（较新在后）；请判断与 agent_reply_plan 是否矛盾。"
    } else {
        "以下为逆序收集的最近工具结果摘要（工作区未设置，可能较不完整）；请判断与 agent_reply_plan 是否矛盾。"
    };
    Some(format!("{}\n{}", header, lines.join("\n")))
}

fn assistant_index_for_tool_call(
    messages: &[Message],
    tool_idx: usize,
    tool_call_id: &str,
) -> Option<usize> {
    for j in (0..tool_idx).rev() {
        if messages[j].role != "assistant" {
            continue;
        }
        let calls = messages[j].tool_calls.as_ref()?;
        if calls.iter().any(|c| c.id == tool_call_id) {
            return Some(j);
        }
    }
    None
}
