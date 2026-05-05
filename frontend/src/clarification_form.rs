//! Web 澄清问卷：SSE 下发后的待填状态（与 `sse_dispatch::ClarificationFormField` 对齐）。

use crate::sse_dispatch::{ClarificationFormField, ClarificationQuestionnaireInfo};

#[derive(Clone)]
pub struct PendingClarificationForm {
    pub questionnaire_id: String,
    pub intro: String,
    pub fields: Vec<ClarificationFormField>,
    /// 与 `fields` 等长；用户输入草稿。
    pub values: Vec<String>,
}

impl PendingClarificationForm {
    pub fn from_sse(info: ClarificationQuestionnaireInfo) -> Self {
        let n = info.fields.len();
        Self {
            questionnaire_id: info.questionnaire_id,
            intro: info.intro,
            fields: info.fields,
            values: vec![String::new(); n],
        }
    }
}
