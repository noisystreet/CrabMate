//! 回合 IO 适配面：把 SSE 控制通道从扁平 [`super::RunLoopIo`] 中切开。
//!
//! - [`TurnControlSink`]：主 SSE `out`、编码器、可选镜像、无 SSE 钩子（工具批 / 澄清问卷）
//!
//! 具体 payload 编码仍在 `execute/tools/emit`；本模块只定**通道形状**，便于入口装配与后续逐步收窄 emit 形参。

use std::sync::Arc;

use tokio::sync::mpsc;

/// 回合控制面输出（Web SSE 主通道 + 可选镜像 / 钩子）。
#[derive(Clone)]
pub(crate) struct TurnControlSink<'a> {
    pub out: Option<&'a mpsc::Sender<String>>,
    /// SSE 编码器：当前为 v1，后续可切换为 v2（AG-UI）。
    pub sse_encoder: Arc<dyn crate::sse::SseEncoder>,
    /// 无 `/chat/stream` 通道时镜像控制面（与 Web [`crate::sse::SsePayload`] 同形）。
    pub sse_control_mirror: Option<crate::sse::SseControlMirror>,
    /// 无 SSE（`out` 为 `None`）时：工具批开始/结束（`true` / `false`）。
    pub tool_running_hook: Option<Arc<dyn Fn(bool) + Send + Sync>>,
    /// 澄清问卷：工具 `present_clarification_questionnaire` 成功时回调。
    pub clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
}
