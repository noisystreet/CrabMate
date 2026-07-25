//! TUI transcript：每回合独立 `section`；流式只对 live 按行 patch（Phase 2）。

use crate::i18n::Locale;
use crate::markdown::plaintext_to_safe_html;
use crate::storage::{StoredMessage, StoredMessageState};
use crate::stream_text_overlay::{
    StreamTextOverlay, message_text_for_display_including_stream_overlay,
};

use super::tui_line_markdown::{
    TuiBodyChunks, TuiBodyPatch, parse_tui_body_chunks, plan_tui_body_patch,
};

/// 上一帧挂载状态。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TuiMountState {
    pub session_id: String,
    /// 已挂载回合 id（顺序与 DOM 一致）。
    pub mounted_ids: Vec<String>,
    pub committed_key: u64,
    pub live_id: Option<String>,
    pub live_body: Option<TuiBodyChunks>,
}

/// 一次 Effect 的 DOM 计划（可组合：先 promote/append，再 live / refresh patch）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TuiSyncPlan {
    pub next: TuiMountState,
    /// 非空则整段替换 transcript，忽略其余局部字段。
    pub full_html: Option<String>,
    pub promote_id: Option<String>,
    pub append_sections: Vec<String>,
    /// 同结构下刷新已挂载回合 body（不拆 section）。
    pub refresh_bodies: Vec<LiveBodyPlan>,
    pub live: Option<LiveBodyPlan>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LiveBodyPlan {
    pub message_id: String,
    pub patch: TuiBodyPatch,
}

fn message_finalize_open_line(message: &StoredMessage) -> bool {
    !message
        .state
        .as_ref()
        .is_some_and(StoredMessageState::is_loading)
}

fn tui_role_label(message: &StoredMessage, locale: Locale) -> String {
    if message.is_tool {
        return message
            .tool_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .map_or_else(
                || crate::i18n::msg_role_tool(locale).to_string(),
                |name| format!("{}:{name}", crate::i18n::msg_role_tool(locale)),
            );
    }
    match message.role.as_str() {
        "user" => crate::i18n::msg_role_user(locale),
        "assistant" => crate::i18n::msg_role_assistant(locale),
        "system" => crate::i18n::msg_role_system(locale),
        _ => crate::i18n::msg_role_other(locale),
    }
    .to_string()
}

#[must_use]
pub(crate) fn live_message_id(
    messages: &[StoredMessage],
    overlay: Option<&StreamTextOverlay>,
) -> Option<String> {
    if let Some(overlay) = overlay
        && messages.iter().any(|message| {
            message.id == overlay.message_id
                && message
                    .state
                    .as_ref()
                    .is_some_and(StoredMessageState::is_loading)
        })
    {
        return Some(overlay.message_id.clone());
    }
    messages
        .iter()
        .rev()
        .find(|message| {
            message
                .state
                .as_ref()
                .is_some_and(StoredMessageState::is_loading)
        })
        .map(|message| message.id.clone())
}

#[must_use]
pub(crate) fn committed_fingerprint(messages: &[StoredMessage], live_id: Option<&str>) -> u64 {
    let mut fingerprint = messages.len() as u64;
    for message in messages {
        if live_id.is_some_and(|id| id == message.id) {
            continue;
        }
        fingerprint = fingerprint.wrapping_mul(41);
        fingerprint = fingerprint.wrapping_add(message.id.len() as u64);
        fingerprint = fingerprint.wrapping_add(message.text.len() as u64);
        fingerprint = fingerprint.wrapping_add(message.reasoning_text.len() as u64);
        fingerprint = fingerprint.wrapping_add(u64::from(message.is_tool));
        if let Some(state) = &message.state {
            fingerprint = fingerprint.wrapping_add(state.to_wire().len() as u64);
        }
        for ch in message.id.bytes() {
            fingerprint = fingerprint.wrapping_mul(31).wrapping_add(u64::from(ch));
        }
    }
    fingerprint
}

fn message_display_text(
    message: &StoredMessage,
    session_id: &str,
    overlay: Option<&StreamTextOverlay>,
    locale: Locale,
    apply_assistant_display_filters: bool,
) -> String {
    message_text_for_display_including_stream_overlay(
        message,
        overlay,
        session_id,
        locale,
        apply_assistant_display_filters,
    )
}

fn message_body_chunks(
    message: &StoredMessage,
    session_id: &str,
    overlay: Option<&StreamTextOverlay>,
    locale: Locale,
    apply_assistant_display_filters: bool,
) -> TuiBodyChunks {
    let text = message_display_text(
        message,
        session_id,
        overlay,
        locale,
        apply_assistant_display_filters,
    );
    parse_tui_body_chunks(&text, message_finalize_open_line(message))
}

fn turn_section_html(
    message: &StoredMessage,
    session_id: &str,
    overlay: Option<&StreamTextOverlay>,
    locale: Locale,
    apply_assistant_display_filters: bool,
    is_live: bool,
) -> String {
    let role = tui_role_label(message, locale);
    let body = message_body_chunks(
        message,
        session_id,
        overlay,
        locale,
        apply_assistant_display_filters,
    )
    .to_inner_html();
    let live_attr = if is_live { " data-tui-live=\"1\"" } else { "" };
    format!(
        "<section class=\"chat-tui-turn\" data-tui-msg-id=\"{}\"{live_attr}>\
         <div class=\"chat-tui-role\">{}</div>\
         <div class=\"chat-tui-body\">{body}</div>\
         </section>",
        plaintext_to_safe_html(&message.id),
        plaintext_to_safe_html(&format!("{role} ❯")),
    )
}

#[must_use]
pub(crate) fn build_tui_transcript_html(
    messages: &[StoredMessage],
    session_id: &str,
    overlay: Option<&StreamTextOverlay>,
    locale: Locale,
    apply_assistant_display_filters: bool,
) -> String {
    if messages.is_empty() {
        return empty_transcript_html(locale);
    }
    let live_id = live_message_id(messages, overlay);
    let mut html = String::new();
    for message in messages {
        let is_live = live_id.as_deref() == Some(message.id.as_str());
        html.push_str(&turn_section_html(
            message,
            session_id,
            overlay,
            locale,
            apply_assistant_display_filters,
            is_live,
        ));
    }
    html
}

fn empty_transcript_html(locale: Locale) -> String {
    format!(
        "<div class=\"chat-tui-empty\">{}</div>",
        plaintext_to_safe_html(crate::i18n::chat_tui_empty(locale))
    )
}

fn ids_prefix(mounted: &[String], messages: &[StoredMessage]) -> bool {
    if mounted.len() > messages.len() {
        return false;
    }
    mounted
        .iter()
        .zip(messages.iter())
        .all(|(id, message)| id == &message.id)
}

fn full_rebuild_plan(
    messages: &[StoredMessage],
    session_id: &str,
    overlay: Option<&StreamTextOverlay>,
    locale: Locale,
    apply_assistant_display_filters: bool,
) -> TuiSyncPlan {
    let live_id = live_message_id(messages, overlay);
    let committed_key = committed_fingerprint(messages, live_id.as_deref());
    let mounted_ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();
    let live_body = live_id.as_ref().and_then(|id| {
        messages.iter().find(|m| &m.id == id).map(|m| {
            message_body_chunks(
                m,
                session_id,
                overlay,
                locale,
                apply_assistant_display_filters,
            )
        })
    });
    TuiSyncPlan {
        next: TuiMountState {
            session_id: session_id.to_string(),
            mounted_ids,
            committed_key,
            live_id,
            live_body,
        },
        full_html: Some(build_tui_transcript_html(
            messages,
            session_id,
            overlay,
            locale,
            apply_assistant_display_filters,
        )),
        promote_id: None,
        append_sections: Vec::new(),
        refresh_bodies: Vec::new(),
        live: None,
    }
}

fn must_full_rebuild(prev: &TuiMountState, messages: &[StoredMessage], session_id: &str) -> bool {
    prev.session_id != session_id
        || messages.is_empty()
        || !ids_prefix(&prev.mounted_ids, messages)
        || prev.mounted_ids.len() > messages.len()
}

fn same_turn_ids(prev: &TuiMountState, messages: &[StoredMessage]) -> bool {
    prev.mounted_ids.len() == messages.len()
        && prev
            .mounted_ids
            .iter()
            .zip(messages.iter())
            .all(|(id, message)| id == &message.id)
}

struct TuiRenderCtx<'a> {
    session_id: &'a str,
    overlay: Option<&'a StreamTextOverlay>,
    locale: Locale,
    apply_filters: bool,
}

fn append_new_turn_sections(
    prev: &TuiMountState,
    messages: &[StoredMessage],
    live_id: Option<&str>,
    ctx: &TuiRenderCtx<'_>,
) -> Vec<String> {
    messages[prev.mounted_ids.len()..]
        .iter()
        .map(|message| {
            turn_section_html(
                message,
                ctx.session_id,
                ctx.overlay,
                ctx.locale,
                ctx.apply_filters,
                live_id == Some(message.id.as_str()),
            )
        })
        .collect()
}

fn promote_id_from(prev: &TuiMountState, live_id: Option<&str>) -> Option<String> {
    prev.live_id
        .as_ref()
        .filter(|prev_live| live_id != Some(prev_live.as_str()))
        .cloned()
}

fn plan_live_patch(
    prev: &TuiMountState,
    messages: &[StoredMessage],
    live_id: Option<&str>,
    promote_id: Option<&str>,
    ctx: &TuiRenderCtx<'_>,
) -> Option<LiveBodyPlan> {
    if let Some(id) = live_id {
        let message = messages.iter().find(|m| m.id == id)?;
        if !prev.mounted_ids.iter().any(|mid| mid == id) {
            return None;
        }
        let next_chunks = message_body_chunks(
            message,
            ctx.session_id,
            ctx.overlay,
            ctx.locale,
            ctx.apply_filters,
        );
        let prev_chunks = prev
            .live_body
            .as_ref()
            .filter(|_| prev.live_id.as_deref() == Some(id));
        return Some(LiveBodyPlan {
            message_id: id.to_string(),
            patch: plan_tui_body_patch(prev_chunks, &next_chunks),
        });
    }
    let pid = promote_id?;
    let message = messages.iter().find(|m| m.id == pid)?;
    let chunks = message_body_chunks(
        message,
        ctx.session_id,
        ctx.overlay,
        ctx.locale,
        ctx.apply_filters,
    );
    Some(LiveBodyPlan {
        message_id: pid.to_string(),
        patch: TuiBodyPatch::ReplaceAll { chunks },
    })
}

fn plan_refresh_bodies(
    messages: &[StoredMessage],
    live_id: Option<&str>,
    promote_id: Option<&str>,
    ctx: &TuiRenderCtx<'_>,
) -> Vec<LiveBodyPlan> {
    messages
        .iter()
        .filter(|message| live_id != Some(message.id.as_str()))
        .filter(|message| promote_id != Some(message.id.as_str()))
        .map(|message| {
            let chunks = message_body_chunks(
                message,
                ctx.session_id,
                ctx.overlay,
                ctx.locale,
                ctx.apply_filters,
            );
            LiveBodyPlan {
                message_id: message.id.clone(),
                patch: TuiBodyPatch::ReplaceAll { chunks },
            }
        })
        .collect()
}

fn next_mount_state(
    messages: &[StoredMessage],
    live_id: Option<String>,
    committed_key: u64,
    ctx: &TuiRenderCtx<'_>,
) -> TuiMountState {
    let live_body = live_id.as_ref().and_then(|id| {
        messages.iter().find(|m| &m.id == id).map(|m| {
            message_body_chunks(
                m,
                ctx.session_id,
                ctx.overlay,
                ctx.locale,
                ctx.apply_filters,
            )
        })
    });
    TuiMountState {
        session_id: ctx.session_id.to_string(),
        mounted_ids: messages.iter().map(|m| m.id.clone()).collect(),
        committed_key,
        live_id,
        live_body,
    }
}

/// 规划 transcript DOM 更新。
#[must_use]
pub(crate) fn plan_tui_sync(
    prev: Option<&TuiMountState>,
    messages: &[StoredMessage],
    session_id: &str,
    overlay: Option<&StreamTextOverlay>,
    locale: Locale,
    apply_assistant_display_filters: bool,
) -> TuiSyncPlan {
    let Some(prev) = prev else {
        return full_rebuild_plan(
            messages,
            session_id,
            overlay,
            locale,
            apply_assistant_display_filters,
        );
    };
    if must_full_rebuild(prev, messages, session_id) {
        return full_rebuild_plan(
            messages,
            session_id,
            overlay,
            locale,
            apply_assistant_display_filters,
        );
    }

    let ctx = TuiRenderCtx {
        session_id,
        overlay,
        locale,
        apply_filters: apply_assistant_display_filters,
    };
    let live_id = live_message_id(messages, overlay);
    let committed_key = committed_fingerprint(messages, live_id.as_deref());
    let append_sections = append_new_turn_sections(prev, messages, live_id.as_deref(), &ctx);
    let promote_id = promote_id_from(prev, live_id.as_deref());
    let live = plan_live_patch(
        prev,
        messages,
        live_id.as_deref(),
        promote_id.as_deref(),
        &ctx,
    );
    let next = next_mount_state(messages, live_id.clone(), committed_key, &ctx);

    let structural_noop = append_sections.is_empty() && promote_id.is_none();
    if structural_noop && prev.committed_key == committed_key && prev.live_id == live_id {
        return TuiSyncPlan {
            next,
            full_html: None,
            promote_id: None,
            append_sections: Vec::new(),
            refresh_bodies: Vec::new(),
            live,
        };
    }

    if same_turn_ids(prev, messages) && append_sections.is_empty() {
        let refresh_bodies =
            plan_refresh_bodies(messages, live_id.as_deref(), promote_id.as_deref(), &ctx);
        return TuiSyncPlan {
            next,
            full_html: None,
            promote_id,
            append_sections: Vec::new(),
            refresh_bodies,
            live,
        };
    }

    TuiSyncPlan {
        next,
        full_html: None,
        promote_id,
        append_sections,
        refresh_bodies: Vec::new(),
        live,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessageState;
    use crate::stream_text_overlay::StreamTextOverlay;

    fn message(id: &str, role: &str, text: &str) -> StoredMessage {
        StoredMessage {
            id: id.to_string(),
            role: role.to_string(),
            text: text.to_string(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn streaming_tokens_incremental_open_plain() {
        let user = message("u1", "user", "hi");
        let mut assistant = message("a1", "assistant", "");
        assistant.state = Some(StoredMessageState::Loading);
        let messages = vec![user, assistant];
        let overlay1 = StreamTextOverlay {
            session_id: "s1".to_string(),
            message_id: "a1".to_string(),
            answer: "**a".to_string(),
            reasoning: String::new(),
        };
        let plan1 = plan_tui_sync(
            None,
            &messages,
            "s1",
            Some(&overlay1),
            Locale::ZhHans,
            false,
        );
        assert!(plan1.full_html.is_some());

        let overlay2 = StreamTextOverlay {
            session_id: "s1".to_string(),
            message_id: "a1".to_string(),
            answer: "**ab".to_string(),
            reasoning: String::new(),
        };
        let plan2 = plan_tui_sync(
            Some(&plan1.next),
            &messages,
            "s1",
            Some(&overlay2),
            Locale::ZhHans,
            false,
        );
        assert!(plan2.full_html.is_none());
        assert!(plan2.append_sections.is_empty());
        let live = plan2.live.expect("live patch");
        assert_eq!(live.message_id, "a1");
        match live.patch {
            TuiBodyPatch::Incremental {
                append_closed,
                open_plain,
            } => {
                assert!(append_closed.is_empty());
                assert_eq!(open_plain.as_deref(), Some("**ab"));
            }
            other => panic!("expected Incremental, got {other:?}"),
        }
    }

    #[test]
    fn append_user_turn_without_full_rebuild() {
        let user = message("u1", "user", "hi");
        let plan1 = plan_tui_sync(
            None,
            std::slice::from_ref(&user),
            "s1",
            None,
            Locale::ZhHans,
            false,
        );
        assert!(plan1.full_html.is_some());

        let mut assistant = message("a1", "assistant", "");
        assistant.state = Some(StoredMessageState::Loading);
        let messages = vec![user, assistant];
        let overlay = StreamTextOverlay {
            session_id: "s1".to_string(),
            message_id: "a1".to_string(),
            answer: String::new(),
            reasoning: String::new(),
        };
        let plan2 = plan_tui_sync(
            Some(&plan1.next),
            &messages,
            "s1",
            Some(&overlay),
            Locale::ZhHans,
            false,
        );
        assert!(plan2.full_html.is_none(), "should append section");
        assert_eq!(plan2.append_sections.len(), 1);
        assert!(plan2.append_sections[0].contains("data-tui-live=\"1\""));
        assert!(
            plan2.live.is_none(),
            "new live body already in section html"
        );
    }

    #[test]
    fn session_switch_forces_full_rebuild() {
        let messages = vec![message("u1", "user", "hi")];
        let plan1 = plan_tui_sync(None, &messages, "s1", None, Locale::ZhHans, false);
        let plan2 = plan_tui_sync(
            Some(&plan1.next),
            &messages,
            "s2",
            None,
            Locale::ZhHans,
            false,
        );
        assert!(plan2.full_html.is_some());
    }

    #[test]
    fn finished_assistant_bold_becomes_strong() {
        let messages = vec![
            message("u1", "user", "你好"),
            message("a1", "assistant", "**原样**"),
        ];
        let output = build_tui_transcript_html(&messages, "s1", None, Locale::ZhHans, false);
        assert!(output.contains("data-tui-msg-id"), "got {output}");
        assert!(
            output.contains("<strong>") || output.contains("<b>"),
            "got {output}"
        );
    }
}
