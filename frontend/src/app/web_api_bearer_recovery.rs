//! 浏览器保存 Web API Bearer 后：重试 `/status`、清鉴权类错误、再拉 LLM 覆盖与会话水合。

use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::client_llm_storage::hydrate_client_llm_from_server;
use crate::api::{is_web_api_credential_error, web_api_bearer_token_is_set};
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;

/// 监听 [`crate::app::app_signals::ShellUISignals::web_api_bearer_save_nonce`]。
///
/// 仅应对**非空** Bearer 保存（设置页清空时不递增 nonce）。若 nonce 被误触发且本页 Bearer
/// 已空，则只刷新 `/status`，不清错误、不拉水合。
pub fn wire_web_api_bearer_save_recovery(
    save_nonce: RwSignal<u64>,
    refresh_status: Arc<dyn Fn() + Send + Sync>,
    status_err: RwSignal<Option<String>>,
    chat: ChatSessionSignals,
    locale: RwSignal<Locale>,
    client_llm_storage_tick: RwSignal<u64>,
) {
    Effect::new(move |_| {
        let n = save_nonce.get();
        if n == 0 {
            return;
        }
        refresh_status();
        if !web_api_bearer_token_is_set() {
            return;
        }
        let clear_auth_err = status_err
            .with_untracked(|e| e.as_ref().is_some_and(|s| is_web_api_credential_error(s)));
        if clear_auth_err {
            status_err.set(None);
        }
        chat.session_hydrate_nonce
            .update(|v| *v = v.saturating_add(1));
        client_llm_storage_tick.update(|t| *t = t.saturating_add(1));
        let loc = locale.get_untracked();
        spawn_local(async move {
            hydrate_client_llm_from_server(loc).await;
        });
    });
}
