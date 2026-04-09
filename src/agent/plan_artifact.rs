//! 最终回答中的结构化「规划」产物：从 assistant content 中解析 JSON，替代 `## 规划` 等子串匹配。

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// 规划步骤 `id` / 可选 `workflow_node_id` 的语法：稳定、可日志引用，并与工作流节点 `id` 常见字符集对齐。
static PLAN_STEP_ID_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9][-A-Za-z0-9_./]{0,127}$")
        .expect("PLAN_STEP_ID_PATTERN: 编译期正则须合法")
});

fn plan_step_id_syntax_ok(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty() && t.len() <= 128 && PLAN_STEP_ID_PATTERN.is_match(t)
}

/// 约定的规划 JSON：`type` + `version` + `steps`；若 `no_task` 为 true 则表示无具体可拆任务，`steps` 须为空。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AgentReplyPlanV1 {
    #[serde(rename = "type")]
    pub plan_type: String,
    pub version: u32,
    pub steps: Vec<PlanStepV1>,
    /// 为 true：模型判定用户未提出需分步执行的具体任务；此时 `steps` 必须为空。
    #[serde(default)]
    pub no_task: bool,
}

/// 分阶段规划单步「子代理」角色：收窄该步内外层循环可见的 **OpenAI tools 列表**，并在执行层拒绝越权 `tool_calls`（与 `write_effect_tools` / 只读判定一致）。
/// 省略或 `null` 表示不限制（与历史行为一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepExecutorKind {
    /// 仅允许语义只读工具（`is_readonly_tool`）；禁止 MCP 代理工具。
    ReviewReadonly,
    /// 只读工具 + 受限写补丁类（`apply_patch` / `search_replace` / `structured_patch` / `create_file` / `modify_file` / `append_file` / `format_file` / `ast_grep_rewrite`）。
    PatchWrite,
    /// 只读工具 + 常见测试运行器（如 `cargo_test` / `pytest_run` / `go_test` 等）；**不含**任意 `run_command`。
    TestRunner,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PlanStepV1 {
    pub id: String,
    pub description: String,
    /// 可选：对应最近一次 `workflow_validate_only` 结果中 `nodes[].id`，供机器校验与轨迹对齐。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_node_id: Option<String>,
    /// 可选：本步执行子循环的工具角色（子代理）；省略则全量工具。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor_kind: Option<PlanStepExecutorKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanArtifactError {
    /// 未找到可解析且通过校验的 JSON 块
    NotFound,
    WrongType(String),
    WrongVersion(u32),
    EmptySteps,
    /// `no_task` 为 true 时 `steps` 必须为空。
    NoTaskWithNonEmptySteps,
    InvalidStep {
        index: usize,
        reason: &'static str,
    },
    /// `workflow_node_id` 已出现但未能覆盖 `workflow_node_ids` 中的全部节点 id（严格 PER 模式）。
    WorkflowNodesNotFullyCovered {
        missing: Vec<String>,
    },
}

/// [`staged_plan_invalid_run_agent_turn_error`] 返回串的固定前缀；供测试、`chat_job_queue` 历史分支识别（**勿**与用户输入拼接）。当前主路径在规划 JSON 无效时已降级为常规循环，一般不再产生该串。
pub(crate) const STAGED_PLAN_INVALID_RUN_AGENT_TURN_ERROR_PREFIX: &str = "staged_plan_invalid:";

/// 供日志单行输出：`WrongType` 仅记长度与短预览，不记完整 `type` 字符串。
pub(crate) fn plan_artifact_error_log_summary(e: &PlanArtifactError) -> String {
    match e {
        PlanArtifactError::NotFound => "not_found".to_string(),
        PlanArtifactError::WrongType(t) => {
            let n = t.chars().count();
            let prev = crate::redact::preview_chars(t, 24);
            format!("wrong_type type_len={n} type_preview={prev}")
        }
        PlanArtifactError::WrongVersion(v) => format!("wrong_version version={v}"),
        PlanArtifactError::EmptySteps => "empty_steps".to_string(),
        PlanArtifactError::NoTaskWithNonEmptySteps => "no_task_with_steps".to_string(),
        PlanArtifactError::InvalidStep { index, reason } => {
            format!("invalid_step index={index} reason={reason}")
        }
        PlanArtifactError::WorkflowNodesNotFullyCovered { missing } => {
            let n = missing.len();
            let prev = missing
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(",");
            format!("workflow_nodes_not_fully_covered missing_count={n} missing_preview={prev}")
        }
    }
}

/// 分阶段规划轮解析失败时的错误串（含结构化摘要）；主路径已改为降级，本函数供单测与兼容识别保留。
#[allow(dead_code)]
pub(crate) fn staged_plan_invalid_run_agent_turn_error(e: PlanArtifactError) -> String {
    format!(
        "{} {}",
        STAGED_PLAN_INVALID_RUN_AGENT_TURN_ERROR_PREFIX,
        plan_artifact_error_log_summary(&e)
    )
}

pub(crate) fn is_staged_plan_invalid_run_agent_turn_error(msg: &str) -> bool {
    msg.starts_with(STAGED_PLAN_INVALID_RUN_AGENT_TURN_ERROR_PREFIX)
}

/// Plan v1 的 schema 规则描述（中文），供提示词引用。
pub const PLAN_V1_SCHEMA_RULES: &str = "\
- 顶层 \"type\" 为字符串 \"agent_reply_plan\"
- \"version\" 为数字 1
- 可选布尔 \"no_task\"：为 true 时表示用户未提出需分步执行的具体任务，此时 \"steps\" 必须为 []（空数组）
- 当 \"no_task\" 省略或为 false 时，\"steps\" 为非空数组；每项含非空字符串 \"id\" 与 \"description\"
- 每项 \"id\" 须唯一；**首尾不得含空白**；语法为 ASCII 字母或数字开头，仅含 - _ . /，总长不超过 128（与 workflow 节点 id 常见字符集一致）
- 可选 \"workflow_node_id\"：若填写，须**首尾无空白**且满足与 \"id\" 相同的语法，且在**同一条规划**中唯一；值应对应最近一次 `workflow_validate_only` 工具结果里 `nodes[].id` 之一（运行时会校验子集）。在严格模式下，若**任一步**填写了 `workflow_node_id`，则**每一个**上述节点 id 都须在步骤中至少出现一次（可合并多 id 到一步时仍须逐 id 引用）
- 可选 \"executor_kind\"（字符串，省略则本步不限制工具）：`review_readonly`（仅只读工具）、`patch_write`（只读 + 受限补丁写）、`test_runner`（只读 + 内置测试运行器）；越权调用会在工具层被拒绝并记入对话";

/// Plan v1 的 JSON 示例。
pub const PLAN_V1_EXAMPLE_JSON: &str = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"layer-0","description":"先执行无依赖节点 …","workflow_node_id":"fmt","executor_kind":"review_readonly"}]}"#;

/// 从整段 assistant `content` 中提取并校验 v1 规划（支持 \`\`\`json / \`\`\`markdown / \`\`\`md 等带语言行的围栏，或整段即为单个 JSON 对象）。
/// 分阶段执行中：当前步工具未全部成功时，将模型返回的**补丁规划**与未完成步之后缀合并。
/// `failed_step_index` 为**零基**（对应 `plan.steps` 下标）；补丁的 `steps` 替换自该步起的后缀。
/// 将合法 v1 规划序列化为单行 JSON（供分阶段规划轮/补丁助手消息写入历史）。
pub(crate) fn agent_reply_plan_v1_to_json_string(
    plan: &AgentReplyPlanV1,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(plan)
}

pub(crate) fn merge_staged_plan_steps_after_step_failure(
    base: &[PlanStepV1],
    patch: &AgentReplyPlanV1,
    failed_step_index: usize,
) -> Result<Vec<PlanStepV1>, PlanArtifactError> {
    if patch.no_task {
        return Err(PlanArtifactError::InvalidStep {
            index: 0,
            reason: "staged_patch_no_task",
        });
    }
    if patch.steps.is_empty() {
        return Err(PlanArtifactError::EmptySteps);
    }
    if failed_step_index >= base.len() {
        return Err(PlanArtifactError::InvalidStep {
            index: failed_step_index,
            reason: "failed_step_index out of range",
        });
    }
    let mut out = Vec::with_capacity(failed_step_index + patch.steps.len());
    out.extend_from_slice(&base[..failed_step_index]);
    out.extend(patch.steps.iter().cloned());
    Ok(out)
}

pub fn parse_agent_reply_plan_v1(content: &str) -> Result<AgentReplyPlanV1, PlanArtifactError> {
    for slice in collect_json_candidates(content) {
        let Ok(plan) = serde_json::from_str::<AgentReplyPlanV1>(&slice) else {
            continue;
        };
        if validate_agent_reply_plan_v1(&plan).is_ok() {
            return Ok(plan);
        }
    }
    Err(PlanArtifactError::NotFound)
}

#[allow(dead_code)] // `per_coord::content_has_plan` 等封装使用
pub fn content_has_valid_agent_reply_plan_v1(content: &str) -> bool {
    parse_agent_reply_plan_v1(content).is_ok()
}

/// 校验规划中出现的 `workflow_node_id` 均为 `workflow_node_ids` 的子集（通常来自最近一次 `workflow_execute` 工具结果的 `nodes[].id`）。
pub(crate) fn validate_plan_workflow_node_ids_subset(
    plan: &AgentReplyPlanV1,
    workflow_node_ids: &[String],
) -> Result<(), PlanArtifactError> {
    let set: HashSet<&str> = workflow_node_ids.iter().map(|s| s.as_str()).collect();
    for (i, s) in plan.steps.iter().enumerate() {
        let Some(ref w) = s.workflow_node_id else {
            continue;
        };
        let w = w.trim();
        if !set.contains(w) {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "workflow_node_id 不在最近一次工作流工具结果的 nodes 列表中",
            });
        }
    }
    Ok(())
}

/// 若规划中**至少一步**含 `workflow_node_id`，则 `steps` 中出现的 `workflow_node_id` 须**覆盖** `workflow_node_ids` 全部节点（每 id 至少一步引用）。
pub(crate) fn validate_plan_covers_all_workflow_node_ids(
    plan: &AgentReplyPlanV1,
    workflow_node_ids: &[String],
) -> Result<(), PlanArtifactError> {
    if workflow_node_ids.is_empty() {
        return Ok(());
    }
    let any_linked = plan.steps.iter().any(|s| s.workflow_node_id.is_some());
    if !any_linked {
        return Ok(());
    }
    let mut covered: HashSet<&str> = HashSet::new();
    for s in &plan.steps {
        if let Some(ref w) = s.workflow_node_id {
            covered.insert(w.trim());
        }
    }
    let mut missing = Vec::new();
    for id in workflow_node_ids {
        let t = id.as_str();
        if !covered.contains(t) {
            missing.push(t.to_string());
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(PlanArtifactError::WorkflowNodesNotFullyCovered { missing })
    }
}

fn validate_agent_reply_plan_v1(p: &AgentReplyPlanV1) -> Result<(), PlanArtifactError> {
    if p.plan_type != "agent_reply_plan" {
        return Err(PlanArtifactError::WrongType(p.plan_type.clone()));
    }
    if p.version != 1 {
        return Err(PlanArtifactError::WrongVersion(p.version));
    }
    if p.no_task {
        if !p.steps.is_empty() {
            return Err(PlanArtifactError::NoTaskWithNonEmptySteps);
        }
        return Ok(());
    }
    if p.steps.is_empty() {
        return Err(PlanArtifactError::EmptySteps);
    }
    let mut seen_step_ids = HashSet::<String>::new();
    let mut seen_workflow_node_ids = HashSet::<String>::new();
    for (i, s) in p.steps.iter().enumerate() {
        let raw_id = s.id.as_str();
        if raw_id != raw_id.trim() {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "id 首尾不得含空白",
            });
        }
        let id = raw_id.trim();
        if id.is_empty() {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "id 为空",
            });
        }
        if !plan_step_id_syntax_ok(id) {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "id 语法不合法（须 ASCII 字母数字起头，仅含 - _ . /，总长不超过 128）",
            });
        }
        if !seen_step_ids.insert(id.to_string()) {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "id 重复",
            });
        }
        if s.description.trim().is_empty() {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "description 为空",
            });
        }
        if let Some(ref w) = s.workflow_node_id {
            let raw_w = w.as_str();
            if raw_w != raw_w.trim() {
                return Err(PlanArtifactError::InvalidStep {
                    index: i,
                    reason: "workflow_node_id 首尾不得含空白",
                });
            }
            let w = raw_w.trim();
            if w.is_empty() {
                return Err(PlanArtifactError::InvalidStep {
                    index: i,
                    reason: "workflow_node_id 若出现须为非空字符串（否则请省略该字段）",
                });
            }
            if !plan_step_id_syntax_ok(w) {
                return Err(PlanArtifactError::InvalidStep {
                    index: i,
                    reason: "workflow_node_id 语法不合法",
                });
            }
            if !seen_workflow_node_ids.insert(w.to_string()) {
                return Err(PlanArtifactError::InvalidStep {
                    index: i,
                    reason: "workflow_node_id 重复",
                });
            }
        }
    }
    Ok(())
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

/// 首个 \`\`\` 代码围栏之前的正文（模型常在此写任务概括），与 JSON 块合读作「目标」。
pub(crate) fn prose_before_first_fence(content: &str) -> String {
    if !content.contains("```") {
        return String::new();
    }
    content.split("```").next().unwrap_or("").trim().to_string()
}

/// 将已解析的 v1 规划格式化为与终答展示相同的 Markdown 有序列表（`1. \`id\`: description`），供 TUI/CLI 分步通知与气泡展示共用。
pub fn format_plan_steps_markdown(plan: &AgentReplyPlanV1) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    let mut n = 1usize;
    for st in &plan.steps {
        let desc = crate::text_sanitize::naturalize_plan_step_description(&st.description);
        let id = st.id.trim();
        if id.is_empty() {
            continue;
        }
        let id_esc = id.replace('`', "'");
        let _ = writeln!(&mut out, "{}. `{}`: {}", n, id_esc, desc.trim());
        n += 1;
    }
    out.trim_end().to_string()
}

/// 分阶段规划**队列**区：每步前加 `[ ]` 未完成 / `[✓]` 已完成（`completed_count` 为已完成步数，与 `run_staged_plan_then_execute_steps` 中步下标一致）。
/// 仅展示 `description`（**不**输出 `step.id`，便于阅读；`Message` / `debug!` 等仍含 id）。
pub fn format_plan_steps_markdown_for_staged_queue(
    plan: &AgentReplyPlanV1,
    completed_count: usize,
) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    let n = plan.steps.len();
    let done = completed_count.min(n);
    let mut line_no = 1usize;
    for (idx, st) in plan.steps.iter().enumerate() {
        let desc = crate::text_sanitize::naturalize_plan_step_description(&st.description);
        let id = st.id.trim();
        if id.is_empty() {
            continue;
        }
        let mark = if idx < done { "[✓]" } else { "[ ]" };
        let _ = writeln!(&mut out, "{mark} {}. {}", line_no, desc.trim());
        line_no += 1;
    }
    out.trim_end().to_string()
}

/// 模型常把「以下是任务拆解」写在首步 `description`，围栏前只有开场白；隐藏 JSON 时该句会随步骤列表一并从主气泡消失。若围栏前 goal 未同时含「以下」与「拆解」，则从首步描述里取**一行**简短引导拼到 goal 前。
pub(crate) fn augment_agent_reply_plan_goal_for_display(
    goal: &str,
    plan: &AgentReplyPlanV1,
) -> String {
    let goal = goal.trim();
    let lead = breakdown_lead_line_from_first_step_description(plan);
    let Some(lead) = lead else {
        return goal.to_string();
    };
    if goal.contains("以下") && goal.contains("拆解") {
        return goal.to_string();
    }
    if goal.is_empty() {
        return lead;
    }
    if goal.contains(&lead) {
        return goal.to_string();
    }
    format!("{lead}\n\n{goal}")
}

fn breakdown_lead_line_from_first_step_description(plan: &AgentReplyPlanV1) -> Option<String> {
    let st = plan.steps.first()?;
    let desc = crate::text_sanitize::naturalize_plan_step_description(&st.description);
    let desc = desc.trim();
    if desc.is_empty() {
        return None;
    }
    for line in desc.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(frag) = extract_task_breakdown_lead_fragment(t) {
            return Some(frag);
        }
    }
    None
}

/// 从首步描述的一行里取出「以下是…拆解」类短引导，在句末标点或冒号处截断，避免把整步描述拼进主气泡。
fn extract_task_breakdown_lead_fragment(line: &str) -> Option<String> {
    let t = line.trim();
    if t.is_empty() {
        return None;
    }
    let candidate = t.contains("以下是任务拆解")
        || (t.contains("以下") && t.contains("拆解") && t.chars().count() <= 72);
    if !candidate {
        return None;
    }
    let start = t.find("以下")?;
    let tail = t[start..].trim_start();
    let clipped = clip_task_breakdown_lead_at_punct(tail);
    let clipped = clipped.trim();
    if clipped.chars().count() < 4 {
        return None;
    }
    Some(cap_display_fragment(clipped, 80))
}

fn clip_task_breakdown_lead_at_punct(s: &str) -> &str {
    for (i, c) in s.char_indices() {
        if matches!(c, '。' | '！' | '？') {
            return &s[..i + c.len_utf8()];
        }
    }
    for (i, c) in s.char_indices() {
        if c == '：' {
            return &s[..i + c.len_utf8()];
        }
    }
    s
}

fn cap_display_fragment(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

/// 若 `content` 中含合法 `agent_reply_plan` v1，返回**简单 Markdown 有序列表**（可选围栏前自然语言段落；每步一行 `1. \`id\`: description`），不含原始 JSON。
/// 仅影响展示层；`Message.content` 仍为原文以便服务端继续解析。
pub fn format_agent_reply_plan_for_display(content: &str) -> Option<String> {
    let plan = parse_agent_reply_plan_v1(content).ok()?;
    let raw_goal = prose_before_first_fence(content);
    let goal = crate::text_sanitize::naturalize_assistant_plan_prose_tail(&raw_goal);
    let goal_t = augment_agent_reply_plan_goal_for_display(goal.trim(), &plan);
    let steps = format_plan_steps_markdown(&plan);
    let mut out = String::new();
    if !goal_t.is_empty() {
        out.push_str(&goal_t);
        out.push_str("\n\n");
    }
    out.push_str(&steps);
    Some(out.trim_end().to_string())
}

/// 围栏内首段非空行若为 `json` / `markdown` / `md`（忽略大小写），返回剥去该行及前导空行后的正文（已 trim）；否则 `None`。
///
/// 与流式缓冲判定共用：无语言行时**不得**把「正文以 `{` 开头」当作规划 JSON（避免裸 \`\`\` 思维链误判）。
pub(crate) fn fenced_body_after_optional_jsonish_lang_label(raw: &str) -> Option<String> {
    let lines: Vec<&str> = raw.lines().collect();
    let mut i = 0usize;
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }
    if i >= lines.len() {
        return None;
    }
    let first_t = lines[i].trim();
    if first_t.eq_ignore_ascii_case("json")
        || first_t.eq_ignore_ascii_case("markdown")
        || first_t.eq_ignore_ascii_case("md")
    {
        Some(lines[i + 1..].join("\n").trim().to_string())
    } else {
        None
    }
}

/// 围栏内首段非空行若为 `json` / `markdown` / `md`（忽略大小写），则剥去该行及之前的前导空行；否则返回 `raw.trim()`。
pub(crate) fn strip_optional_json_fence_label(raw: &str) -> String {
    fenced_body_after_optional_jsonish_lang_label(raw).unwrap_or_else(|| raw.trim().to_string())
}

fn fence_inner_should_hide_agent_reply_plan_json(inner: &str) -> bool {
    let raw = inner.trim();
    let body = strip_optional_json_fence_label(raw);
    if !body.starts_with('{') {
        return false;
    }
    if parse_agent_reply_plan_v1(&body).is_ok() {
        return true;
    }
    let b = body.trim();
    // 流式输出时半截 JSON 往往已含 "agent_reply_plan" / "steps" 子串；若仅凭子串就隐去围栏，
    // 展示层会在 `assistant_markdown_source_for_display` 里得到空串，用户看不到围栏前的说明句。
    // 仅当围栏内已是**语法闭合**的 JSON 值、却仍不能通过 v1 规划校验时，再按形状隐去原文。
    if !b.contains("\"agent_reply_plan\"") || !b.contains("\"steps\"") {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(b).is_ok()
}

/// 从展示用正文中移除含 `agent_reply_plan` 的 Markdown 代码围栏块（``` … ```），
/// 不修改 `Message.content`；日志仍可用 `debug!` 打印原文。
///
/// **未闭合且围栏内尚为空**（流式刚打出起始 ` ``` `、尚未写入语言行或正文）：不伪造收尾围栏。否则 `inner == ""` 时会拼出连续六个反引号（` ``` `` + `` + ``` `）。
pub fn strip_agent_reply_plan_fence_blocks_for_display(content: &str) -> String {
    let parts: Vec<&str> = content.split("```").collect();
    let unclosed_trailing_fence = parts.len().is_multiple_of(2);
    let mut out = String::new();
    let mut i = 0usize;
    while i < parts.len() {
        out.push_str(parts[i]);
        i += 1;
        if i >= parts.len() {
            break;
        }
        let inner = parts[i];
        i += 1;
        if fence_inner_should_hide_agent_reply_plan_json(inner) {
            continue;
        }
        if unclosed_trailing_fence && i >= parts.len() && inner.trim().is_empty() {
            break;
        }
        out.push_str("```");
        out.push_str(inner);
        out.push_str("```");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json() -> String {
        r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"do a"}]}"#
            .to_string()
    }

    #[test]
    fn staged_plan_invalid_error_prefix_and_detector() {
        let e = staged_plan_invalid_run_agent_turn_error(PlanArtifactError::NotFound);
        assert!(is_staged_plan_invalid_run_agent_turn_error(&e));
        assert!(e.starts_with(STAGED_PLAN_INVALID_RUN_AGENT_TURN_ERROR_PREFIX));
    }

    #[test]
    fn merge_staged_plan_steps_replaces_suffix_from_failed_index() {
        let base = vec![
            PlanStepV1 {
                id: "s0".into(),
                description: "done".into(),
                workflow_node_id: None,
                executor_kind: None,
            },
            PlanStepV1 {
                id: "s1".into(),
                description: "fail".into(),
                workflow_node_id: None,
                executor_kind: None,
            },
            PlanStepV1 {
                id: "s2".into(),
                description: "old tail".into(),
                workflow_node_id: None,
                executor_kind: None,
            },
        ];
        let patch = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![
                PlanStepV1 {
                    id: "s1b".into(),
                    description: "retry".into(),
                    workflow_node_id: None,
                    executor_kind: None,
                },
                PlanStepV1 {
                    id: "s2b".into(),
                    description: "new tail".into(),
                    workflow_node_id: None,
                    executor_kind: None,
                },
            ],
            no_task: false,
        };
        let merged = merge_staged_plan_steps_after_step_failure(&base, &patch, 1).unwrap();
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].id, "s0");
        assert_eq!(merged[1].id, "s1b");
        assert_eq!(merged[2].id, "s2b");
    }

    #[test]
    fn plan_artifact_error_log_summary_redacts_long_wrong_type() {
        let long = "x".repeat(80);
        let s = plan_artifact_error_log_summary(&PlanArtifactError::WrongType(long.clone()));
        assert!(!s.contains(&long));
        assert!(s.contains("type_len=80"));
    }

    #[test]
    fn validate_plan_covers_all_workflow_node_ids_gate() {
        let ids = vec!["a".to_string(), "b".to_string()];
        let no_link = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![PlanStepV1 {
                id: "s1".into(),
                description: "x".into(),
                workflow_node_id: None,
                executor_kind: None,
            }],
            no_task: false,
        };
        assert!(validate_plan_covers_all_workflow_node_ids(&no_link, &ids).is_ok());
        let partial = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![PlanStepV1 {
                id: "s1".into(),
                description: "x".into(),
                workflow_node_id: Some("a".into()),
                executor_kind: None,
            }],
            no_task: false,
        };
        assert!(validate_plan_covers_all_workflow_node_ids(&partial, &ids).is_err());
        let full = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![
                PlanStepV1 {
                    id: "s1".into(),
                    description: "x".into(),
                    workflow_node_id: Some("a".into()),
                    executor_kind: None,
                },
                PlanStepV1 {
                    id: "s2".into(),
                    description: "y".into(),
                    workflow_node_id: Some("b".into()),
                    executor_kind: None,
                },
            ],
            no_task: false,
        };
        assert!(validate_plan_covers_all_workflow_node_ids(&full, &ids).is_ok());
    }

    #[test]
    fn strip_fence_removes_plan_json_keeps_prose() {
        let bad = r#"{"type":"agent_reply_plan","version":1,"steps":[]}"#;
        let content = format!("说明\n```json\n{bad}\n```\n");
        let s = strip_agent_reply_plan_fence_blocks_for_display(&content);
        assert!(s.contains("说明"));
        assert!(!s.contains("agent_reply_plan"));
    }

    #[test]
    fn strip_fence_keeps_streaming_incomplete_plan_inside_fence() {
        let partial =
            r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"x""#;
        let content = format!("先说明一句。\n```json\n{partial}");
        let s = strip_agent_reply_plan_fence_blocks_for_display(&content);
        assert!(s.contains("先说明一句"));
        assert!(
            s.contains("agent_reply_plan"),
            "语法未闭合且 inner 非空时仍保留围栏内流式正文（并带伪造收尾 ```，由上层缓冲抑制刷屏）"
        );
    }

    #[test]
    fn parses_fenced_json() {
        let content = format!("说明\n```json\n{}\n```\n", sample_json());
        let p = parse_agent_reply_plan_v1(&content).unwrap();
        assert_eq!(p.steps.len(), 1);
        assert_eq!(p.steps[0].id, "a");
    }

    #[test]
    fn rejects_step_id_bad_syntax() {
        let bad =
            r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":" bad","description":"x"}]}"#;
        let content = format!("```json\n{bad}\n```");
        assert!(parse_agent_reply_plan_v1(&content).is_err());
    }

    #[test]
    fn validate_workflow_node_id_subset() {
        let plan = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![PlanStepV1 {
                id: "s1".into(),
                description: "do".into(),
                workflow_node_id: Some("fmt".into()),
                executor_kind: None,
            }],
            no_task: false,
        };
        assert!(validate_plan_workflow_node_ids_subset(&plan, &["fmt".into()]).is_ok());
        assert!(validate_plan_workflow_node_ids_subset(&plan, &["other".into()]).is_err());
    }

    #[test]
    fn parses_fenced_markdown_wrapped_plan_json() {
        let content = format!("说明\n```markdown\n{}\n```\n", sample_json());
        let p = parse_agent_reply_plan_v1(&content).unwrap();
        assert_eq!(p.steps.len(), 1);
        assert_eq!(p.steps[0].id, "a");
    }

    #[test]
    fn strip_fence_removes_plan_json_markdown_fence() {
        let j = sample_json();
        let content = format!("说明\n```markdown\n{j}\n```\n");
        let s = strip_agent_reply_plan_fence_blocks_for_display(&content);
        assert!(s.contains("说明"));
        assert!(!s.contains("agent_reply_plan"));
        assert!(!s.contains("```"));
    }

    #[test]
    fn strip_fence_unclosed_opening_does_not_emit_six_backticks() {
        let s = strip_agent_reply_plan_fence_blocks_for_display("说明\n```");
        assert_eq!(s, "说明\n");
        assert!(!s.contains("```"));
    }

    #[test]
    fn augment_goal_prepends_breakdown_lead_when_only_in_first_step() {
        let step_json = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"以下是任务拆解。创建 hello.cpp"}]}"#;
        let content = format!("让我先规划一下任务步骤：\n```json\n{step_json}\n```\n");
        let plan = parse_agent_reply_plan_v1(&content).unwrap();
        let raw = prose_before_first_fence(&content);
        let goal = crate::text_sanitize::naturalize_assistant_plan_prose_tail(&raw);
        let out = augment_agent_reply_plan_goal_for_display(goal.trim(), &plan);
        assert!(
            out.contains("以下是任务拆解"),
            "应拼回首步里的拆解引导句: {}",
            out
        );
        assert!(out.contains("让我先规划"), "{}", out);
    }

    #[test]
    fn parses_raw_json_only_message() {
        let p = parse_agent_reply_plan_v1(&sample_json()).unwrap();
        assert_eq!(p.plan_type, "agent_reply_plan");
    }

    #[test]
    fn parses_executor_kind_on_step() {
        let j = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"r","description":"审","executor_kind":"review_readonly"},{"id":"p","description":"改","executor_kind":"patch_write"},{"id":"t","description":"测","executor_kind":"test_runner"}]}"#;
        let p = parse_agent_reply_plan_v1(j).unwrap();
        assert_eq!(
            p.steps[0].executor_kind,
            Some(PlanStepExecutorKind::ReviewReadonly)
        );
        assert_eq!(
            p.steps[1].executor_kind,
            Some(PlanStepExecutorKind::PatchWrite)
        );
        assert_eq!(
            p.steps[2].executor_kind,
            Some(PlanStepExecutorKind::TestRunner)
        );
    }

    #[test]
    fn rejects_legacy_heading() {
        let content = "## 规划\n- step one";
        assert!(parse_agent_reply_plan_v1(content).is_err());
    }

    #[test]
    fn rejects_wrong_type() {
        let s = r#"{"type":"other","version":1,"steps":[{"id":"x","description":"y"}]}"#;
        assert!(parse_agent_reply_plan_v1(s).is_err());
    }

    #[test]
    fn rejects_empty_steps() {
        let s = r#"{"type":"agent_reply_plan","version":1,"steps":[]}"#;
        assert!(parse_agent_reply_plan_v1(s).is_err());
    }

    #[test]
    fn parses_no_task_empty_steps() {
        let s = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#;
        let p = parse_agent_reply_plan_v1(s).unwrap();
        assert!(p.no_task);
        assert!(p.steps.is_empty());
    }

    #[test]
    fn rejects_no_task_with_non_empty_steps() {
        let s = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[{"id":"a","description":"x"}]}"#;
        assert!(parse_agent_reply_plan_v1(s).is_err());
    }

    #[test]
    fn rejects_bad_json_in_fence_then_accepts_second() {
        let content = r#"
```json
not json
```
```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"1","description":"ok"}]}
```
"#;
        assert!(parse_agent_reply_plan_v1(content).is_ok());
    }

    #[test]
    fn staged_queue_marks_done_steps() {
        let plan = parse_agent_reply_plan_v1(
            r#"{"type":"agent_reply_plan","version":1,"steps":[
                {"id":"a","description":"one"},
                {"id":"b","description":"two"}
            ]}"#,
        )
        .unwrap();
        let s0 = format_plan_steps_markdown_for_staged_queue(&plan, 0);
        assert!(s0.contains("[ ]"));
        assert!(!s0.contains("[✓]"));
        let s1 = format_plan_steps_markdown_for_staged_queue(&plan, 1);
        assert!(s1.lines().next().unwrap_or("").contains("[✓]"));
        assert!(s1.lines().nth(1).unwrap_or("").contains("[ ]"));
        let s2 = format_plan_steps_markdown_for_staged_queue(&plan, 2);
        assert_eq!(s2.matches("[✓]").count(), 2);
    }

    #[test]
    fn format_display_includes_goal_and_steps() {
        let content = "先调研再改代码。\n```json\n{\"type\":\"agent_reply_plan\",\"version\":1,\"steps\":[{\"id\":\"s1\",\"description\":\"读 README\"},{\"id\":\"s2\",\"description\":\"改 main\"}]}\n```\n";
        let s = format_agent_reply_plan_for_display(content).expect("formatted");
        assert!(s.contains("调研"));
        assert!(s.contains("1. `s1`: 读 README"));
        assert!(s.contains("2. `s2`: 改 main"));
        assert!(!s.contains("agent_reply_plan"));
    }

    #[test]
    fn format_display_raw_json_only_still_works() {
        let content =
            r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"do"}]}"#;
        let s = format_agent_reply_plan_for_display(content).expect("formatted");
        assert_eq!(s, "1. `a`: do");
    }
}
