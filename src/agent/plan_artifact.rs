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
}
