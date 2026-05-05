//! 消息行上的**副作用动作**（分支 API、本地截断后再流式）：从 `message_row` 视图拆出，降低视图文件与 `api`/`session_ops` 的纠缠。

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{ChatBranchError, post_chat_branch};
use crate::chat_actions::apply_branch_success_revision;
use crate::i18n::Locale;
use crate::session_ops::{
    truncate_at_user_message_and_prepare_regenerate, truncate_at_user_message_branch_local,
    user_ordinal_for_message_index,
};
use crate::session_sync::SessionSyncState;
use crate::storage::ChatSession;

fn dbg_log(msg: &str) {
    web_sys::console::log_1(&format!("[regen] {msg}").into());
}

fn dbg_log_with(msg: &str, val: &str) {
    web_sys::console::log_2(&format!("[regen] {}: {}", msg, val).into(), &val.into());
}

/// 用户消息上「再生 / 分支」按钮所需的信号子集（[`Copy`]，便于在 `view!` 闭包中捕获）。
#[derive(Clone, Copy)]
pub(crate) struct MessageRowActionSignals {
    pub session_sync: RwSignal<SessionSyncState>,
    pub sessions: RwSignal<Vec<ChatSession>>,
    pub active_id: RwSignal<String>,
    pub regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    pub status_err: RwSignal<Option<String>>,
    pub locale: RwSignal<Locale>,
}

/// 工具卡「跳转到本回合用户提问」：关自动跟底并滚到锚点。
pub(crate) fn spawn_scroll_to_linked_user_message(uid: &str, auto_scroll_chat: RwSignal<bool>) {
    auto_scroll_chat.set(false);
    let u = uid.to_string();
    spawn_local(async move {
        TimeoutFuture::new(32).await;
        crate::session_search::scroll_message_into_view(&u);
    });
}

impl MessageRowActionSignals {
    /// 「在用户消息后重新生成」：`POST /chat/branch`（若有会话 revision）或仅本地截断并触发 `regen_stream_after_truncate`。
    pub(crate) fn spawn_regenerate_from_user_line(self, msg_idx: usize, user_message_id: String) {
        let MessageRowActionSignals {
            session_sync,
            sessions,
            active_id,
            regen_stream_after_truncate,
            status_err,
            locale,
        } = self;

        let (cid, rev) = session_sync.with(|s| {
            let (a, b) = s.branch_id_and_expected_revision();
            (a.map(|x| x.to_string()), b)
        });
        dbg_log_with("cid", &cid.clone().unwrap_or_default());
        dbg_log_with("rev", &rev.map(|r| r.to_string()).unwrap_or_default());
        let ord = sessions.with(|list| {
            let aid = active_id.get_untracked();
            list.iter()
                .find(|s| s.id == aid)
                .and_then(|s| user_ordinal_for_message_index(&s.messages, msg_idx))
        });
        dbg_log_with("ord", &ord.map(|o| o.to_string()).unwrap_or_default());
        let uid = user_message_id;
        match (cid, rev, ord) {
            (Some(conv), Some(exp_rev), Some(before_ord)) => {
                dbg_log("PATH A: post_chat_branch (cid + rev + ord all Some)");
                let loc = locale.get_untracked();
                spawn_local(async move {
                    match post_chat_branch(&conv, before_ord, exp_rev, loc).await {
                        Ok(new_rev) => {
                            dbg_log_with("post_chat_branch ok, new_rev", &new_rev.to_string());
                            let aid = active_id.get_untracked();
                            apply_branch_success_revision(session_sync, sessions, &aid, new_rev);
                            let mut prep: Option<(String, Vec<String>, String)> = None;
                            sessions.update(|list| {
                                prep = truncate_at_user_message_and_prepare_regenerate(
                                    list, &aid, &uid,
                                );
                            });
                            if let Some((ut, uimg, aid)) = prep {
                                dbg_log_with("PATH A prep set regen_stream", &ut);
                                regen_stream_after_truncate.set(Some((ut, uimg, aid)));
                            } else {
                                dbg_log("PATH A prep was None");
                            }
                        }
                        Err(e) => {
                            dbg_log_with("post_chat_branch err", e.as_deref());
                            match &e {
                                ChatBranchError::NotFound => {
                                    // 会话在服务端不存在（server 重启/过期）：清除绑定，直接本地重试。
                                    session_sync.update(|s| s.invalidate_conversation_id());
                                    let mut prep: Option<(String, Vec<String>, String)> = None;
                                    sessions.update(|list| {
                                        let aid = active_id.get_untracked();
                                        prep = truncate_at_user_message_and_prepare_regenerate(
                                            list, &aid, &uid,
                                        );
                                    });
                                    if let Some((ut, uimg, aid)) = prep {
                                        dbg_log_with("NotFound PATH B prep set regen_stream", &ut);
                                        regen_stream_after_truncate.set(Some((ut, uimg, aid)));
                                    }
                                }
                                ChatBranchError::Conflict => {
                                    session_sync.update(|s| s.mark_branch_conflict());
                                    status_err.set(Some(
                                        crate::i18n::api_err_branch_failed(
                                            crate::i18n::Locale::ZhHans,
                                        )
                                        .to_string(),
                                    ));
                                }
                                ChatBranchError::Other(_) => {
                                    session_sync.update(|s| s.mark_branch_conflict());
                                    status_err.set(Some(e.as_deref().to_string()));
                                }
                            }
                        }
                    }
                });
            }
            _ => {
                dbg_log("PATH B: local-only (cid or rev or ord is None)");
                let mut prep: Option<(String, Vec<String>, String)> = None;
                sessions.update(|list| {
                    let aid = active_id.get_untracked();
                    prep = truncate_at_user_message_and_prepare_regenerate(list, &aid, &uid);
                });
                if let Some((ut, uimg, aid)) = prep {
                    dbg_log_with("PATH B prep set regen_stream", &ut);
                    regen_stream_after_truncate.set(Some((ut, uimg, aid)));
                } else {
                    dbg_log("PATH B prep was None");
                }
            }
        }
    }

    /// 「从用户消息分支」：服务端分支或仅本地截断视图。
    pub(crate) fn spawn_branch_at_user_line(self, msg_idx: usize, user_message_id: String) {
        let MessageRowActionSignals {
            session_sync,
            sessions,
            active_id,
            regen_stream_after_truncate: _,
            status_err,
            locale,
        } = self;

        let (cid, rev) = session_sync.with(|s| {
            let (a, b) = s.branch_id_and_expected_revision();
            (a.map(|x| x.to_string()), b)
        });
        let ord = sessions.with(|list| {
            let aid = active_id.get_untracked();
            list.iter()
                .find(|s| s.id == aid)
                .and_then(|s| user_ordinal_for_message_index(&s.messages, msg_idx))
        });
        let uid = user_message_id;
        match (cid, rev, ord) {
            (Some(conv), Some(exp_rev), Some(before_ord)) => {
                let loc_b = locale.get_untracked();
                spawn_local(async move {
                    match post_chat_branch(&conv, before_ord, exp_rev, loc_b).await {
                        Ok(new_rev) => {
                            let aid = active_id.get_untracked();
                            apply_branch_success_revision(session_sync, sessions, &aid, new_rev);
                            sessions.update(|list| {
                                let _ = truncate_at_user_message_branch_local(list, &aid, &uid);
                            });
                        }
                        Err(e) => {
                            let err_display = match &e {
                                ChatBranchError::NotFound => {
                                    session_sync.update(|s| s.invalidate_conversation_id());
                                    crate::i18n::api_err_branch_failed(crate::i18n::Locale::ZhHans)
                                        .to_string()
                                }
                                ChatBranchError::Conflict | ChatBranchError::Other(_) => {
                                    session_sync.update(|s| s.mark_branch_conflict());
                                    e.as_deref().to_string()
                                }
                            };
                            status_err.set(Some(err_display));
                        }
                    }
                });
            }
            _ => {
                sessions.update(|list| {
                    let aid = active_id.get_untracked();
                    let _ = truncate_at_user_message_branch_local(list, &aid, &uid);
                });
            }
        }
    }
}
