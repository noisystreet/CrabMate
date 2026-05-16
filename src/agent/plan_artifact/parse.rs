use super::fence::strip_optional_json_fence_label;
use super::types::{
    AgentReplyPlanV1, PlanArtifactError, normalize_agent_reply_plan_v1_acceptance_in_place,
};
use super::validate::{
    validate_agent_reply_plan_v1, validate_agent_reply_plan_v1_with_validate_only_binding_ids,
};

/// 解析 `agent_reply_plan` v1（**无 validate-only 绑定上下文**）。
///
/// 注意：在需要执行 `workflow_validate_only` 绑定语义的路径中，应优先使用
/// [`parse_agent_reply_plan_v1_with_validate_only_binding_ids`]，避免误放行多步绑定例外。
pub fn parse_agent_reply_plan_v1(content: &str) -> Result<AgentReplyPlanV1, PlanArtifactError> {
    for slice in collect_json_candidates(content) {
        let Ok(plan) = serde_json::from_str::<AgentReplyPlanV1>(&slice) else {
            continue;
        };
        if validate_agent_reply_plan_v1(&plan).is_ok() {
            let mut plan = plan;
            normalize_agent_reply_plan_v1_acceptance_in_place(&mut plan);
            return Ok(plan);
        }
    }
    Err(PlanArtifactError::NotFound)
}

/// 与 [`parse_agent_reply_plan_v1`] 类似，但在「多步 + 每步 `workflow_node_id`」例外上
/// 要求显式存在 validate-only 绑定上下文（`validate_only_binding_ids` 非空）。
pub(crate) fn parse_agent_reply_plan_v1_with_validate_only_binding_ids(
    content: &str,
    validate_only_binding_ids: Option<&[String]>,
) -> Result<AgentReplyPlanV1, PlanArtifactError> {
    for slice in collect_json_candidates(content) {
        let Ok(plan) = serde_json::from_str::<AgentReplyPlanV1>(&slice) else {
            continue;
        };
        if validate_agent_reply_plan_v1_with_validate_only_binding_ids(
            &plan,
            validate_only_binding_ids,
        )
        .is_ok()
        {
            let mut plan = plan;
            normalize_agent_reply_plan_v1_acceptance_in_place(&mut plan);
            return Ok(plan);
        }
    }
    Err(PlanArtifactError::NotFound)
}

/// 合并 `reasoning_details`→`reasoning_content` 后与正文拼接，供 [`parse_agent_reply_plan_v1`] 使用（网关常把 `agent_reply_plan` 放在思维链字段）。
pub(crate) fn assistant_merged_text_for_plan_artifact_parse(msg: &crate::types::Message) -> String {
    let mut m = msg.clone();
    crate::types::merge_reasoning_details_into_reasoning_content(&mut m);
    let c = crate::types::message_content_as_str(&m.content).unwrap_or("");
    let r = m.reasoning_content.as_deref().unwrap_or("");
    let c = c.trim();
    let r = r.trim();
    match (r.is_empty(), c.is_empty()) {
        (true, true) => String::new(),
        (true, false) => c.to_string(),
        (false, true) => r.to_string(),
        (false, false) => format!("{r}\n\n{c}"),
    }
}

/// 从助手消息（正文 + 思维链）解析 `agent_reply_plan` v1。
pub fn parse_agent_reply_plan_v1_from_assistant_message(
    msg: &crate::types::Message,
) -> Result<AgentReplyPlanV1, PlanArtifactError> {
    let merged = assistant_merged_text_for_plan_artifact_parse(msg);
    parse_agent_reply_plan_v1(&merged)
}

/// 从助手消息（正文 + 思维链）解析 `agent_reply_plan` v1（带 validate-only 绑定上下文）。
pub(crate) fn parse_agent_reply_plan_v1_from_assistant_message_with_validate_only_binding_ids(
    msg: &crate::types::Message,
    validate_only_binding_ids: Option<&[String]>,
) -> Result<AgentReplyPlanV1, PlanArtifactError> {
    let merged = assistant_merged_text_for_plan_artifact_parse(msg);
    parse_agent_reply_plan_v1_with_validate_only_binding_ids(&merged, validate_only_binding_ids)
}

#[allow(dead_code)] // `per_coord::content_has_plan` 等封装使用
pub fn content_has_valid_agent_reply_plan_v1(content: &str) -> bool {
    parse_agent_reply_plan_v1(content).is_ok()
}

/// 候选 JSON 字符串：每个 fenced \`\`\` 块（奇数段）去掉可选语言行（`json` / `markdown` / `md`，忽略大小写；可含前导空行）后尝试；再尝试整段 trim 后以 `{` 开头的全文。
fn collect_json_candidates(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let parts: Vec<&str> = content.split("```").collect();
    for i in (1..parts.len()).step_by(2) {
        let raw = parts[i].trim();
        if raw.is_empty() {
            continue;
        }
        let body = strip_optional_json_fence_label(raw);
        if body.starts_with('{') {
            out.push(body);
        }
    }
    let all = content.trim();
    if all.starts_with('{') && !out.iter().any(|s| s.as_str() == all) {
        out.push(all.to_string());
    }
    out
}
