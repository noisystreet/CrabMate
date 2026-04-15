//! 薄封装：在 **`RunLoopParams`** 上复用 [`crate::llm::CompleteChatRetryingParams`] 拼装，减少分阶段等路径的重复样板。
//! 仍**只**经 [`crate::llm::complete_chat_retrying`] 出站，不直接调 `api::stream_chat`。

use crate::llm::{
    CompleteChatRetryingParams, LlmCompleteError, LlmRetryingTransportOpts, complete_chat_retrying,
};
use crate::types::ChatRequest;

use super::params::RunLoopParams;

/// 绑定一轮 `run_agent_turn` 的传输层与 Chrome trace，便于只改 `out` / `render_to_terminal` 等再调模型。
pub(crate) struct AgentLlmCall<'p> {
    p: &'p RunLoopParams<'p>,
}

impl<'p> AgentLlmCall<'p> {
    #[inline]
    pub(crate) fn new(p: &'p RunLoopParams<'p>) -> Self {
        Self { p }
    }

    /// 使用给定传输选项调用 [`complete_chat_retrying`]。
    pub(crate) async fn complete_retrying(
        &self,
        transport: LlmRetryingTransportOpts<'p>,
        req: &ChatRequest,
    ) -> Result<(crate::types::Message, String), LlmCompleteError> {
        let cc = CompleteChatRetryingParams::new(
            self.p.llm_backend,
            self.p.client,
            self.p.api_key,
            self.p.cfg.as_ref(),
            transport,
            self.p.request_chrome_trace.clone(),
            self.p
                .model_override
                .as_deref()
                .or(self.p.cfg.planner_model.as_deref()),
        );
        complete_chat_retrying(&cc, req).await
    }
}
