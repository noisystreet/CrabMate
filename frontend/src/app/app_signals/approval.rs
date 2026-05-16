//! 审批条、澄清问卷与思维迹日志。

use leptos::prelude::*;

use crate::clarification_form::PendingClarificationForm;
use crate::sse_dispatch::ThinkingTraceInfo;

#[derive(Clone, Copy)]
pub struct ApprovalSignals {
    pub pending_approval: RwSignal<Option<(String, String, String)>>,
    pub approval_expanded: RwSignal<bool>,
    pub last_approval_sid: RwSignal<String>,
    pub pending_clarification: RwSignal<Option<PendingClarificationForm>>,
    pub thinking_trace_log: RwSignal<Vec<ThinkingTraceInfo>>,
}

impl ApprovalSignals {
    pub fn new() -> Self {
        Self {
            pending_approval: RwSignal::new(None),
            approval_expanded: RwSignal::new(false),
            last_approval_sid: RwSignal::new(String::new()),
            pending_clarification: RwSignal::new(None),
            thinking_trace_log: RwSignal::new(Vec::new()),
        }
    }
}

impl Default for ApprovalSignals {
    fn default() -> Self {
        Self::new()
    }
}
