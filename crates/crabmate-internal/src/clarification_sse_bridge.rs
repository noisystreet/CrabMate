//! 澄清问卷解析结果 → SSE 控制面体（`crabmate-tools` 与 `sse::protocol` 桥接）。

use crabmate_tools::clarification_questionnaire::{
    ClarificationQuestionnaireBody as ToolsBody, parse_present_clarification_body,
};

use crate::sse::{ClarificationQuestionField, ClarificationQuestionnaireBody};

fn to_sse_body(body: ToolsBody) -> ClarificationQuestionnaireBody {
    ClarificationQuestionnaireBody {
        questionnaire_id: body.questionnaire_id,
        intro: body.intro,
        questions: body
            .questions
            .into_iter()
            .map(|q| ClarificationQuestionField {
                id: q.id,
                label: q.label,
                hint: q.hint,
                required: q.required,
                kind: q.kind,
            })
            .collect(),
    }
}

/// 工具成功且为澄清问卷时解析出控制面体（与 SSE / TUI 回调共用）。
pub fn clarification_questionnaire_body_if_tool_ok(
    tool_name: &str,
    args_json: &str,
    tool_output: &str,
) -> Option<ClarificationQuestionnaireBody> {
    if tool_name != "present_clarification_questionnaire" {
        return None;
    }
    if !tool_output.trim_start().starts_with("退出码：0") {
        return None;
    }
    parse_present_clarification_body(args_json)
        .ok()
        .map(to_sse_body)
}
