//! 根包 [`crabmate_agent::IntentL2ClassifierHost`] 实现（委托 `intent_l2_classifier`）。

use async_trait::async_trait;

use crabmate_agent::agent_turn::IntentL2ClassifierHost;
use crabmate_agent::intent_pipeline::L2IntentAttempt;
use crabmate_config::AgentConfig;
use crabmate_llm::ChatCompletionsBackend;
use reqwest::Client;

/// 进程内默认 L2 分类宿主（无工具 LLM；失败时携带脱敏原因并兜底）。
pub struct CrabmateIntentL2ClassifierHost<'a> {
    pub cfg: &'a AgentConfig,
    pub llm_backend: &'a dyn ChatCompletionsBackend,
    pub client: &'a Client,
    pub api_key: &'a str,
}

#[async_trait]
impl IntentL2ClassifierHost for CrabmateIntentL2ClassifierHost<'_> {
    async fn classify_l2_attempt(
        &self,
        routing_for_l1: &str,
        current_task: &str,
    ) -> L2IntentAttempt {
        match crate::agent::intent_l2_classifier::classify_intent_l2_with_llm(
            routing_for_l1,
            current_task,
            self.cfg,
            self.llm_backend,
            self.client,
            self.api_key,
        )
        .await
        {
            Ok(candidate) => L2IntentAttempt::from_candidate(Some(candidate)),
            Err(reason) => L2IntentAttempt::unavailable(reason),
        }
    }
}
