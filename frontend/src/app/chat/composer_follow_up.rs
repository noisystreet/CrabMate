//! 截断再生与失败助手重试的**单一待发队列**（显式状态机）。
//!
//! 由 [`super::composer_wires::wire_chat_composer_streams`] 内唯一 `Effect` 消费，取代两个互不协调的 `Option` 信号，避免双 `Effect` 对 `attach` 的隐式顺序依赖。

/// 合成器在「用户未点发送」前提下、待 `/chat/stream` `attach` 的后续动作。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub(crate) enum ComposerStreamFollowUp {
    /// 无待发动作。
    #[default]
    Idle,
    /// 重试当前失败助手气泡：由 [`crate::session_ops::prepare_retry_failed_assistant_turn`] 解析后再 `attach`。
    RetryFailedAssistant { failed_asst_id: String },
    /// 用户线截断后再生：已备好 `(user_text, imgs, loading_asst_id)`，条件满足即 `attach`。
    RegenerateAfterTruncate {
        user_text: String,
        user_imgs: Vec<String>,
        asst_id: String,
    },
}
