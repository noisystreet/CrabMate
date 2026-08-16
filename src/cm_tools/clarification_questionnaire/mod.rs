//! 澄清问卷：模型经内置工具 `present_clarification_questionnaire` 触发 Web SSE，用户下一回合用 `clarify_questionnaire_answers` 回传。

use serde_json::Map;

pub type ClarifyAnswersNormalized = (String, Map<String, serde_json::Value>);

pub(super) const MAX_QUESTIONNAIRE_ID_LEN: usize = 128;
pub(super) const MAX_INTRO_CHARS: usize = 2000;
pub(super) const MAX_QUESTIONS: usize = 12;
pub(super) const MIN_QUESTIONS: usize = 1;
pub(super) const MAX_QUESTION_ID_LEN: usize = 64;
pub(super) const MAX_LABEL_CHARS: usize = 512;
pub(super) const MAX_HINT_CHARS: usize = 512;
pub(super) const MAX_ANSWER_VALUE_CHARS: usize = 4000;
pub(super) const MAX_ANSWER_KEYS: usize = 32;

mod answers;
mod id_validation;
mod present;
#[cfg(test)]
mod tests;

pub use answers::{
    merge_user_text_with_clarification_answers, normalize_clarify_questionnaire_answers_raw,
};
pub use present::{
    ClarificationQuestionField, ClarificationQuestionnaireBody, parse_present_clarification_body,
    run_present_clarification_questionnaire,
};
