use serde_json::{Map, Value};

use super::id_validation::{valid_question_id, valid_questionnaire_id};
use super::{ClarifyAnswersNormalized, MAX_ANSWER_KEYS, MAX_ANSWER_VALUE_CHARS};

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
