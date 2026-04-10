//! 澄清问卷：模型经内置工具 `present_clarification_questionnaire` 触发 Web SSE，用户下一回合用 `clarify_questionnaire_answers` 回传。

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::sse::{
    ClarificationQuestionField, ClarificationQuestionnaireBody, SsePayload, encode_message,
};

const MAX_QUESTIONNAIRE_ID_LEN: usize = 128;
const MAX_INTRO_CHARS: usize = 2000;
const MAX_QUESTIONS: usize = 12;
const MIN_QUESTIONS: usize = 1;
const MAX_QUESTION_ID_LEN: usize = 64;
const MAX_LABEL_CHARS: usize = 512;
const MAX_HINT_CHARS: usize = 512;
const MAX_ANSWER_VALUE_CHARS: usize = 4000;
const MAX_ANSWER_KEYS: usize = 32;

pub(crate) type ClarifyAnswersNormalized = (String, Map<String, Value>);

fn valid_questionnaire_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_QUESTIONNAIRE_ID_LEN
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn valid_question_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_QUESTION_ID_LEN
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

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

/// 若工具为 `present_clarification_questionnaire` 且执行成功，在工具结果 SSE 之后额外下发控制面（不改变写入模型的 tool 正文）。
pub(crate) async fn maybe_emit_clarification_questionnaire_sse(
    out: Option<&tokio::sync::mpsc::Sender<String>>,
    tool_name: &str,
    args_json: &str,
    tool_output: &str,
) {
    if tool_name != "present_clarification_questionnaire" {
        return;
    }
    let Some(tx) = out else {
        return;
    };
    if !tool_output.trim_start().starts_with("退出码：0") {
        return;
    }
    let Ok(body) = parse_present_clarification_body(args_json) else {
        return;
    };
    let _ = crate::sse::send_string_logged(
        tx,
        encode_message(SsePayload::ClarificationQuestionnaire {
            clarification_questionnaire: body,
        }),
        "clarification_questionnaire",
    )
    .await;
}

/// 规范化 `POST /chat*` 中的 `clarify_questionnaire_answers`；非法则 Err（HTTP 400）。
pub(crate) fn normalize_clarify_questionnaire_answers_raw(
    questionnaire_id: String,
    answers: Value,
) -> Result<Option<ClarifyAnswersNormalized>, String> {
    let qid = questionnaire_id.trim().to_string();
    if qid.is_empty() {
        return Ok(None);
    }
    if !valid_questionnaire_id(&qid) {
        return Err("clarify_questionnaire_answers.questionnaire_id 非法".to_string());
    }
    let Some(obj) = answers.as_object().cloned() else {
        return Err("clarify_questionnaire_answers.answers 须为 JSON 对象".to_string());
    };
    if obj.len() > MAX_ANSWER_KEYS {
        return Err(format!(
            "clarify_questionnaire_answers.answers 键过多（上限 {MAX_ANSWER_KEYS}）"
        ));
    }
    let mut out = Map::new();
    for (k, v) in obj {
        let kid = k.trim().to_string();
        if !valid_question_id(&kid) {
            return Err(format!(
                "clarify_questionnaire_answers 中含非法字段名 `{kid}`"
            ));
        }
        let s = match v {
            Value::String(t) => t,
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => {
                return Err(format!(
                    "clarify_questionnaire_answers[`{kid}`] 须为字符串、数字或布尔"
                ));
            }
        };
        if s.chars().count() > MAX_ANSWER_VALUE_CHARS {
            return Err(format!(
                "clarify_questionnaire_answers[`{kid}`] 过长（上限 {MAX_ANSWER_VALUE_CHARS} 字符）"
            ));
        }
        out.insert(kid, Value::String(s));
    }
    Ok(Some((qid, out)))
}

/// 将用户输入与澄清答案合并为送入模型的单条 user 文本（在 `@` 展开之后调用）。
pub(crate) fn merge_user_text_with_clarification_answers(
    expanded_user_text: String,
    clarify: Option<ClarifyAnswersNormalized>,
) -> String {
    let Some((qid, map)) = clarify else {
        return expanded_user_text;
    };
    if map.is_empty() {
        return expanded_user_text;
    }
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    let mut block = String::from("\n\n---\n**澄清问卷回答**（`questionnaire_id=");
    block.push_str(&qid);
    block.push_str("`）\n\n");
    for k in keys {
        let v = map.get(k).and_then(|x| x.as_str()).unwrap_or("");
        block.push_str("- `");
        block.push_str(k);
        block.push_str("`: ");
        block.push_str(v);
        block.push('\n');
    }
    if expanded_user_text.trim().is_empty() {
        block.trim_start().to_string()
    } else {
        format!("{expanded_user_text}{block}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_prepends_block_after_text() {
        let m = serde_json::json!({"a": "x", "b": "y"});
        let obj = m.as_object().unwrap().clone();
        let out =
            merge_user_text_with_clarification_answers("hello".into(), Some(("q1".into(), obj)));
        assert!(out.starts_with("hello"));
        assert!(out.contains("questionnaire_id=q1"));
        assert!(out.contains("`a`: x"));
        assert!(out.contains("`b`: y"));
    }
}
