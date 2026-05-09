//! SSE 控制面载荷形状与回调分组类型（与 **`dispatch`** 子模块中 **`try_dispatch_sse_control_payload`** 的消费契约一致）。

use serde_json::Value;

use crate::i18n::Locale;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseDispatch {
    Stop,
    Handled,
    Plain,
}

/// 工作区与工具相关控制面回调（`tool_call` / `tool_running` / 审批等）。
#[allow(clippy::type_complexity)]
pub struct SseWorkspaceToolHooks<'a> {
    pub on_workspace_changed: Option<&'a mut dyn FnMut()>,
    pub on_tool_call: Option<
        &'a mut dyn FnMut(
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
    >,
    pub on_tool_status_change: Option<&'a mut dyn FnMut(bool)>,
    pub on_parsing_tool_calls_change: Option<&'a mut dyn FnMut(bool)>,
    pub on_tool_result: Option<&'a mut dyn FnMut(ToolResultInfo)>,
    pub on_command_approval_request: Option<&'a mut dyn FnMut(CommandApprovalRequest)>,
}

/// `assistant_answer_phase` 与分步规划时间线。
pub struct SseStagedPlanHooks<'a> {
    /// 后续 `on_delta` 为终答正文（此前为思维链）；无链时也会在首段正文前下发。
    pub on_assistant_answer_phase: Option<&'a mut dyn FnMut()>,
    pub on_staged_plan_step_started: Option<&'a mut dyn FnMut(StagedPlanStepStartInfo)>,
    pub on_staged_plan_step_finished: Option<&'a mut dyn FnMut(StagedPlanStepEndInfo)>,
}

/// 澄清问卷与思维迹调试事件。
pub struct SseClarifyTraceHooks<'a> {
    pub on_clarification_questionnaire: Option<&'a mut dyn FnMut(ClarificationQuestionnaireInfo)>,
    pub on_thinking_trace: Option<&'a mut dyn FnMut(ThinkingTraceInfo)>,
}

/// 会话落盘 revision、`timeline_log`、协议能力等尾部控制面。
pub struct SseNoticeTimelineHooks<'a> {
    /// `conversation_saved.revision`，供 `POST /chat/branch` 与冲突检测。
    pub on_conversation_saved_revision: Option<&'a mut dyn FnMut(u64)>,
    /// `timeline_log` 事件：审批结果等旁注，写入时间线（不进聊天正文）。
    pub on_timeline_log: Option<&'a mut dyn FnMut(TimelineLogInfo)>,
}

/// SSE 控制面分发入口：按领域分组回调，与 `dispatch::try_dispatch_sse_control_payload` 分支顺序对齐。
pub struct SseControlSink<'a> {
    /// 用户可见错误文案语言（如 SSE 协议版本不匹配提示）。
    pub user_locale: Locale,
    pub on_error: &'a mut dyn FnMut(String),
    pub workspace_tool: SseWorkspaceToolHooks<'a>,
    pub staged_plan: SseStagedPlanHooks<'a>,
    pub clarify_trace: SseClarifyTraceHooks<'a>,
    pub notice_timeline: SseNoticeTimelineHooks<'a>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // 与后端 JSON 同形；展示层当前仅用 name/summary。
pub struct ToolResultInfo {
    pub name: String,
    pub goal_id: Option<String>,
    /// 与对应 `tool_call.tool_call_id` 对齐；缺省时前端按 FIFO 与占位气泡配对。
    pub tool_call_id: Option<String>,
    /// 与 `crabmate_tool.v` 对齐；缺省按 **1**（与后端 `serde(default)` 一致）。
    pub result_version: u32,
    pub summary: Option<String>,
    pub output: String,
    pub ok: Option<bool>,
    pub exit_code: Option<i64>,
    pub error_code: Option<String>,
    /// 与 Rust `tool_error::ToolFailureCategory` 蛇形字符串同源（`invalid_input` 等）。
    pub failure_category: Option<String>,
    /// 可选：与 `read_file` / `read_dir` / `list_tree` 工具输出首行 **`crabmate_tool_output`** 同源（SSE 侧复制），便于 UI 表格化。
    pub structured_preview: Option<Value>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CommandApprovalRequest {
    pub command: String,
    pub args: String,
    pub allowlist_key: Option<String>,
}

/// `staged_plan_step_started`：Web 时间线展示用字段子集。
#[derive(Debug, Clone)]
pub struct StagedPlanStepStartInfo {
    pub step_index: usize,
    pub total_steps: usize,
    pub description: String,
    pub executor_kind: Option<String>,
}

/// `staged_plan_step_finished`：Web 时间线展示用字段子集。
#[derive(Debug, Clone)]
pub struct StagedPlanStepEndInfo {
    pub step_index: usize,
    pub total_steps: usize,
    pub status: String,
    pub executor_kind: Option<String>,
}

/// `clarification_questionnaire`：Web 表单用字段子集。
#[derive(Debug, Clone)]
pub struct ClarificationQuestionnaireInfo {
    pub questionnaire_id: String,
    pub intro: String,
    pub fields: Vec<ClarificationFormField>,
}

#[derive(Debug, Clone)]
pub struct ClarificationFormField {
    pub id: String,
    pub label: String,
    pub hint: Option<String>,
    pub required: bool,
}

/// `thinking_trace`：Web 调试台用（不进聊天正文）。
#[derive(Debug, Clone)]
pub struct ThinkingTraceInfo {
    pub op: String,
    pub node_id: Option<String>,
    pub parent_id: Option<String>,
    pub title: Option<String>,
    pub chunk: Option<String>,
    pub context_snapshot: Option<String>,
}

/// `timeline_log`：Web 时间线旁注（审批结果等；不进聊天正文）。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TimelineLogInfo {
    pub kind: String,
    pub title: String,
    pub detail: Option<String>,
}
