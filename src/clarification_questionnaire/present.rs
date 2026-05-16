use serde::Deserialize;

use crate::sse::{ClarificationQuestionField, ClarificationQuestionnaireBody};

use super::{
    MAX_HINT_CHARS, MAX_INTRO_CHARS, MAX_LABEL_CHARS, MAX_QUESTION_ID_LEN, MAX_QUESTIONS,
    MIN_QUESTIONS,
    id_validation::{valid_question_id, valid_questionnaire_id},
};

#[derive(Debug, Deserialize)]
struct PresentArgs {
    questionnaire_id: String,
    intro: String,
    questions: Vec<PresentQuestion>,
}

#[derive(Debug, Deserialize)]
struct PresentQuestion {
    id: String,
    label: String,
    #[serde(default)]
    hint: Option<String>,
    #[serde(default)]
    required: Option<bool>,
    #[serde(default)]
    kind: Option<String>,
}

fn parse_present_clarification_body(
    args_json: &str,
) -> Result<ClarificationQuestionnaireBody, String> {
    let args: PresentArgs =
        serde_json::from_str(args_json).map_err(|e| format!("参数 JSON 无效：{e}"))?;
    let qid = args.questionnaire_id.trim().to_string();
    if !valid_questionnaire_id(&qid) {
        return Err("questionnaire_id 须为非空字母数字与 -_，且长度不超过 128".to_string());
    }
    let intro = args.intro.trim().to_string();
    if intro.is_empty() {
        return Err("intro 不能为空".to_string());
    }
    if intro.chars().count() > MAX_INTRO_CHARS {
        return Err(format!("intro 过长（上限 {MAX_INTRO_CHARS} 字符）"));
    }
    let n = args.questions.len();
    if !(MIN_QUESTIONS..=MAX_QUESTIONS).contains(&n) {
        return Err(format!(
            "questions 数量须在 {MIN_QUESTIONS}～{MAX_QUESTIONS} 之间"
        ));
    }
    let mut fields: Vec<ClarificationQuestionField> = Vec::with_capacity(n);
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for q in args.questions {
        let id = q.id.trim().to_string();
        if !valid_question_id(&id) {
            return Err(format!(
                "题目 id `{id}` 非法（须字母数字与 -_，长度 ≤ {MAX_QUESTION_ID_LEN}）"
            ));
        }
        if !seen.insert(id.clone()) {
            return Err(format!("题目 id `{id}` 重复"));
        }
        let label = q.label.trim().to_string();
        if label.is_empty() || label.chars().count() > MAX_LABEL_CHARS {
            return Err(format!(
                "题目 `{id}` 的 label 须非空且不超过 {MAX_LABEL_CHARS} 字符"
            ));
        }
        let hint = q
            .hint
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if hint
            .as_ref()
            .is_some_and(|h| h.chars().count() > MAX_HINT_CHARS)
        {
            return Err(format!("题目 `{id}` 的 hint 过长（上限 {MAX_HINT_CHARS}）"));
        }
        let kind = match q.kind.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None | Some("text") => Some("text".to_string()),
            Some("choice") => Some("choice".to_string()),
            Some(k) => {
                return Err(format!("题目 `{id}` 的 kind 非法：{k}（仅 text / choice）"));
            }
        };
        fields.push(ClarificationQuestionField {
            id,
            label,
            hint,
            required: q.required,
            kind,
        });
    }
    Ok(ClarificationQuestionnaireBody {
        questionnaire_id: qid,
        intro,
        questions: fields,
    })
}

/// 内置工具 `present_clarification_questionnaire`：校验参数；Web 流式下 `execute_tools` 会据此补发 SSE。
pub(crate) fn run_present_clarification_questionnaire(args_json: &str) -> String {
    match parse_present_clarification_body(args_json) {
        Ok(body) => {
            let mut lines: Vec<String> = Vec::new();
            lines.push("退出码：0".to_string());
            lines.push(format!(
                "已登记澄清问卷 `{}`（共 {} 题）。Web 客户端将收到 `clarification_questionnaire` 控制面事件；用户下一条消息请附带 clarify_questionnaire_answers。",
                body.questionnaire_id,
                body.questions.len()
            ));
            lines.push("---".to_string());
            lines.push(format!("**说明**：{}", body.intro));
            for q in &body.questions {
                let req = if q.required == Some(true) {
                    "（必填）"
                } else {
                    ""
                };
                let hint = q
                    .hint
                    .as_deref()
                    .map(|h| format!(" — {h}"))
                    .unwrap_or_default();
                let k = q.kind.as_deref().unwrap_or("text");
                lines.push(format!("- `{id}`{req} [{k}]{hint}", id = q.id));
                lines.push(format!("  {}", q.label));
            }
            lines.join("\n")
        }
        Err(e) => format!("退出码：1\n{e}\n"),
    }
}

/// 工具成功且为澄清问卷时解析出控制面体（与 SSE / TUI 回调共用）。
pub(crate) fn clarification_questionnaire_body_if_tool_ok(
    tool_name: &str,
    args_json: &str,
    tool_output: &str,
) -> Option<crate::sse::ClarificationQuestionnaireBody> {
    if tool_name != "present_clarification_questionnaire" {
        return None;
    }
    if !tool_output.trim_start().starts_with("退出码：0") {
        return None;
    }
    parse_present_clarification_body(args_json).ok()
}
