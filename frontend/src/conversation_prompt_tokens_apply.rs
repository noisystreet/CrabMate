//! 将 SSE / 水合得到的 tiktoken 快照写入 [`crate::chat_session_state::ConversationPromptTokenHydrate`]。

use leptos::prelude::Set;
use serde_json::Value;

use crate::chat_session_state::{ChatSessionSignals, ConversationPromptTokenHydrate};
use crate::conversation_hydrate::TiktokenPromptTokensSnapshot;

/// 从 `conversation_saved` / `stream_ended` 等控制面 JSON 子对象解析。
pub fn parse_tiktoken_prompt_tokens_value(v: &Value) -> Option<TiktokenPromptTokensSnapshot> {
    serde_json::from_value(v.clone()).ok()
}

/// 流式回合结束或 `conversation_saved` 携带的 tiktoken；`conversation_id` 须与当前绑定会话一致。
pub fn apply_conversation_prompt_tokens_from_sse(
    chat: ChatSessionSignals,
    conversation_id: &str,
    snap: TiktokenPromptTokensSnapshot,
) {
    let cid = conversation_id.trim();
    if cid.is_empty() {
        return;
    }
    chat.conversation_prompt_tokens
        .set(Some(ConversationPromptTokenHydrate {
            conversation_id: cid.to_string(),
            tiktoken: Some(snap),
        }));
}
