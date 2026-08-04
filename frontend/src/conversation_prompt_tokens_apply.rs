//! 将 SSE / 水合得到的 tiktoken 快照写入 [`crate::chat_session_state::ConversationPromptTokenHydrate`]。

use leptos::prelude::Set;
use serde_json::Value;

use crate::chat_session_state::{ChatSessionSignals, ConversationPromptTokenHydrate};
use crate::conversation_hydrate::TiktokenPromptTokensSnapshot;

/// 从 `conversation_saved` / `stream_ended` 等控制面 JSON 子对象解析。
pub fn parse_tiktoken_prompt_tokens_value(v: &Value) -> Option<TiktokenPromptTokensSnapshot> {
    serde_json::from_value(v.clone()).ok()
}

/// 从 AG-UI 事件根或 `CUSTOM conversation_saved` 的 `data` 中读取可选 tiktoken。
///
/// 同时接受 camelCase（`tiktokenPromptTokens`）与 snake_case（`tiktoken_prompt_tokens`）。
#[must_use]
pub fn tiktoken_from_ag_ui_object(obj: &Value) -> Option<TiktokenPromptTokensSnapshot> {
    obj.get("tiktokenPromptTokens")
        .or_else(|| obj.get("tiktoken_prompt_tokens"))
        .and_then(parse_tiktoken_prompt_tokens_value)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_camel_case_and_snake_case_keys() {
        let camel = json!({
            "tiktokenPromptTokens": {
                "prompt_tokens": 1200,
                "tiktoken_model": "gpt-4o"
            }
        });
        let snake = json!({
            "tiktoken_prompt_tokens": {
                "prompt_tokens": 99,
                "tiktoken_model": "gpt-4"
            }
        });
        let a = tiktoken_from_ag_ui_object(&camel).expect("camel");
        assert_eq!(a.prompt_tokens, 1200);
        assert_eq!(a.tiktoken_model, "gpt-4o");
        let b = tiktoken_from_ag_ui_object(&snake).expect("snake");
        assert_eq!(b.prompt_tokens, 99);
        assert!(tiktoken_from_ag_ui_object(&json!({"revision": 1})).is_none());
        assert!(tiktoken_from_ag_ui_object(&json!({"tiktokenPromptTokens": null})).is_none());
    }
}
