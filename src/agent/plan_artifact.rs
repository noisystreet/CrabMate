//! 最终回答中的结构化「规划」产物：从 assistant content 中解析 JSON，替代 `## 规划` 等子串匹配。

use serde::Deserialize;

/// 约定的规划 JSON：`type` + `version` + 非空 `steps`。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AgentReplyPlanV1 {
    #[serde(rename = "type")]
    pub plan_type: String,
    pub version: u32,
    pub steps: Vec<PlanStepV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PlanStepV1 {
    pub id: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanArtifactError {
    /// 未找到可解析且通过校验的 JSON 块
    NotFound,
    WrongType(String),
    WrongVersion(u32),
    EmptySteps,
    InvalidStep {
        index: usize,
        reason: &'static str,
    },
}

/// 从整段 assistant `content` 中提取并校验 v1 规划（支持 \`\`\`json 围栏，或整段即为单个 JSON 对象）。
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

fn validate_agent_reply_plan_v1(p: &AgentReplyPlanV1) -> Result<(), PlanArtifactError> {
    if p.plan_type != "agent_reply_plan" {
        return Err(PlanArtifactError::WrongType(p.plan_type.clone()));
    }
    if p.version != 1 {
        return Err(PlanArtifactError::WrongVersion(p.version));
    }
    if p.steps.is_empty() {
        return Err(PlanArtifactError::EmptySteps);
    }
    for (i, s) in p.steps.iter().enumerate() {
        if s.id.trim().is_empty() {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "id 为空",
            });
        }
        if s.description.trim().is_empty() {
            return Err(PlanArtifactError::InvalidStep {
                index: i,
                reason: "description 为空",
            });
        }
    }
    Ok(())
}

/// 候选 JSON 字符串：每个 fenced \`\`\` 块（奇数段）去掉可选的 `json` 语言行后尝试；再尝试整段 trim 后以 `{` 开头的全文。
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
fn prose_before_first_fence(content: &str) -> String {
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

/// 若 `content` 中含合法 `agent_reply_plan` v1，返回**简单 Markdown 有序列表**（可选围栏前自然语言段落；每步一行 `1. \`id\`: description`），不含原始 JSON。
/// 仅影响展示层；`Message.content` 仍为原文以便服务端继续解析。
pub fn format_agent_reply_plan_for_display(content: &str) -> Option<String> {
    let plan = parse_agent_reply_plan_v1(content).ok()?;
    let raw_goal = prose_before_first_fence(content);
    let goal = crate::text_sanitize::naturalize_assistant_plan_prose_tail(&raw_goal);
    let goal_t = goal.trim();
    let steps = format_plan_steps_markdown(&plan);
    let mut out = String::new();
    if !goal_t.is_empty() {
        out.push_str(goal_t);
        out.push_str("\n\n");
    }
    out.push_str(&steps);
    Some(out.trim_end().to_string())
}

fn strip_optional_json_fence_label(raw: &str) -> String {
    let mut lines = raw.lines();
    let Some(first) = lines.next() else {
        return raw.trim().to_string();
    };
    let first_t = first.trim();
    if first_t.eq_ignore_ascii_case("json") {
        lines.collect::<Vec<_>>().join("\n").trim().to_string()
    } else {
        raw.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json() -> String {
        r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"do a"}]}"#
            .to_string()
    }

    #[test]
    fn parses_fenced_json() {
        let content = format!("说明\n```json\n{}\n```\n", sample_json());
        let p = parse_agent_reply_plan_v1(&content).unwrap();
        assert_eq!(p.steps.len(), 1);
        assert_eq!(p.steps[0].id, "a");
    }

    #[test]
    fn parses_raw_json_only_message() {
        let p = parse_agent_reply_plan_v1(&sample_json()).unwrap();
        assert_eq!(p.plan_type, "agent_reply_plan");
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
