use super::{MAX_QUESTION_ID_LEN, MAX_QUESTIONNAIRE_ID_LEN};

pub(super) fn valid_questionnaire_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_QUESTIONNAIRE_ID_LEN
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub(super) fn valid_question_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_QUESTION_ID_LEN
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}
