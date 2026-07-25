//! TUI transcript：每回合独立 wrap（section + 操作条）；工具为一行摘要；流式只对 live 按行 patch。

use std::collections::HashMap;

use crate::i18n::Locale;
use crate::markdown::plaintext_to_safe_html;
use crate::storage::{StoredMessage, StoredMessageState};
use crate::stream_text_overlay::{
    StreamTextOverlay, message_text_for_display_including_stream_overlay,
};

use super::tui_actions_bar::turn_actions_bar_html;
use super::tui_line_markdown::{
    TuiBodyChunks, TuiBodyPatch, parse_tui_body_chunks, plan_tui_body_patch,
};
use super::tui_tool_process::{tool_process_body_html, tool_row_live_fields};

/// 上一帧挂载状态。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TuiMountState {
    pub session_id: String,
    /// 已挂载回合 id（顺序与 DOM 一致）。
    pub mounted_ids: Vec<String>,
    pub committed_key: u64,
    pub live_id: Option<String>,
    pub live_body: Option<TuiBodyChunks>,
    /// live 工具行是否已挂载 details（用于决定 ToolRow vs ReplaceAll）。
    pub live_tool_has_details: Option<bool>,
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
    /// 刷新回合下方操作条（状态变化：loading→done / error）。
    pub refresh_actions: Vec<TurnActionsPlan>,
    pub live: Option<LiveBodyPlan>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LiveBodyPlan {
    pub message_id: String,
    pub patch: TuiBodyPatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TurnActionsPlan {
    pub message_id: String,
    pub html: String,
}

fn message_finalize_open_line(message: &StoredMessage) -> bool {
    !message
        .state
        .as_ref()
        .is_some_and(StoredMessageState::is_loading)
}

/// 工具名写在过程行内；不重复角色标签（对齐气泡）。
fn tui_role_label(message: &StoredMessage, locale: Locale) -> String {
    if message.is_tool {
        return String::new();
    }
    crate::session_ops::message_role_label(message, locale).to_string()
}

fn tui_turn_role_class(message: &StoredMessage) -> &'static str {
    if message.is_tool {
        return "chat-tui-turn--tool";
    }
    match message.role.as_str() {
        "user" => "chat-tui-turn--user",
        "assistant" => "chat-tui-turn--assistant",
        "system" => "chat-tui-turn--system",
        _ => "chat-tui-turn--other",
    }
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

fn tool_live_overlay<'a>(
    message: &StoredMessage,
    tool_chunks: &'a HashMap<String, String>,
) -> Option<&'a str> {
    message
        .tool_call_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .and_then(|id| tool_chunks.get(id))
        .map(String::as_str)
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
    tool_chunks: &HashMap<String, String>,
) -> TuiBodyChunks {
    if message.is_tool {
        let live = tool_live_overlay(message, tool_chunks);
        return TuiBodyChunks {
            closed: vec![tool_process_body_html(message, locale, live)],
            open_plain: None,
        };
    }
    let text = message_display_text(
        message,
        session_id,
        overlay,
        locale,
        apply_assistant_display_filters,
    );
    parse_tui_body_chunks(&text, message_finalize_open_line(message))
}

struct TurnSectionArgs<'a> {
    message: &'a StoredMessage,
    msg_idx: usize,
    session_id: &'a str,
    overlay: Option<&'a StreamTextOverlay>,
    locale: Locale,
    apply_filters: bool,
    is_live: bool,
    tool_chunks: &'a HashMap<String, String>,
}

fn turn_section_html(args: TurnSectionArgs<'_>) -> String {
    let TurnSectionArgs {
        message,
        msg_idx,
        session_id,
        overlay,
        locale,
        apply_filters,
        is_live,
        tool_chunks,
    } = args;
    let role = tui_role_label(message, locale);
    let body = message_body_chunks(
        message,
        session_id,
        overlay,
        locale,
        apply_filters,
        tool_chunks,
    )
    .to_inner_html();
    let role_class = tui_turn_role_class(message);
    let live_class = if is_live { " chat-tui-turn--live" } else { "" };
    let loading_class = if message
        .state
        .as_ref()
        .is_some_and(StoredMessageState::is_loading)
    {
        " is-loading"
    } else {
        ""
    };
    let live_attr = if is_live { " data-tui-live=\"1\"" } else { "" };
    let role_block = if role.is_empty() {
        String::new()
    } else {
        format!(
            "<div class=\"chat-tui-role\"><span class=\"chat-tui-role-label\">{}</span></div>",
            plaintext_to_safe_html(&role)
        )
    };
    let id_esc = plaintext_to_safe_html(&message.id);
    // 角色字样在卡片外（wrap 内、section 上），与气泡 msg-meta 外置一致。
    let section = format!(
        "<section class=\"chat-tui-turn {role_class}{live_class}{loading_class}\" data-tui-msg-id=\"{id_esc}\"{live_attr}>\
         <div class=\"chat-tui-body\">{body}</div>\
         </section>"
    );
    let actions = turn_actions_bar_html(message, msg_idx, locale);
    let wrap_align = if role_class == "chat-tui-turn--user" {
        " chat-tui-turn-wrap--user"
    } else {
        ""
    };
    format!(
        "<div class=\"chat-tui-turn-wrap{wrap_align}\" data-tui-wrap-id=\"{id_esc}\">{role_block}{section}{actions}</div>"
    )
}

#[must_use]
pub(crate) fn build_tui_transcript_html(
    messages: &[StoredMessage],
    session_id: &str,
    overlay: Option<&StreamTextOverlay>,
    locale: Locale,
    apply_assistant_display_filters: bool,
    tool_chunks: &HashMap<String, String>,
) -> String {
    if messages.is_empty() {
        return empty_transcript_html(locale);
    }
    let live_id = live_message_id(messages, overlay);
    let mut html = String::new();
    for (msg_idx, message) in messages.iter().enumerate() {
        let is_live = live_id.as_deref() == Some(message.id.as_str());
        html.push_str(&turn_section_html(TurnSectionArgs {
            message,
            msg_idx,
            session_id,
            overlay,
            locale,
            apply_filters: apply_assistant_display_filters,
            is_live,
            tool_chunks,
        }));
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

fn live_tool_has_details_flag(
    messages: &[StoredMessage],
    live_id: Option<&str>,
    locale: Locale,
    tool_chunks: &HashMap<String, String>,
) -> Option<bool> {
    let id = live_id?;
    let message = messages.iter().find(|m| m.id == id)?;
    if !message.is_tool {
        return None;
    }
    let live = tool_live_overlay(message, tool_chunks);
    Some(tool_row_live_fields(message, locale, live).wants_details())
}

fn full_rebuild_plan(
    messages: &[StoredMessage],
    session_id: &str,
    overlay: Option<&StreamTextOverlay>,
    locale: Locale,
    apply_assistant_display_filters: bool,
    tool_chunks: &HashMap<String, String>,
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
                tool_chunks,
            )
        })
    });
    let live_tool_has_details =
        live_tool_has_details_flag(messages, live_id.as_deref(), locale, tool_chunks);
    TuiSyncPlan {
        next: TuiMountState {
            session_id: session_id.to_string(),
            mounted_ids,
            committed_key,
            live_id,
            live_body,
            live_tool_has_details,
        },
        full_html: Some(build_tui_transcript_html(
            messages,
            session_id,
            overlay,
            locale,
            apply_assistant_display_filters,
            tool_chunks,
        )),
        promote_id: None,
        append_sections: Vec::new(),
        refresh_bodies: Vec::new(),
        refresh_actions: Vec::new(),
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
    tool_chunks: &'a HashMap<String, String>,
}

fn append_new_turn_sections(
    prev: &TuiMountState,
    messages: &[StoredMessage],
    live_id: Option<&str>,
    ctx: &TuiRenderCtx<'_>,
) -> Vec<String> {
    messages
        .iter()
        .enumerate()
        .skip(prev.mounted_ids.len())
        .map(|(msg_idx, message)| {
            turn_section_html(TurnSectionArgs {
                message,
                msg_idx,
                session_id: ctx.session_id,
                overlay: ctx.overlay,
                locale: ctx.locale,
                apply_filters: ctx.apply_filters,
                is_live: live_id == Some(message.id.as_str()),
                tool_chunks: ctx.tool_chunks,
            })
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
            ctx.tool_chunks,
        );
        if message.is_tool && prev.live_id.as_deref() == Some(id) {
            let live = tool_live_overlay(message, ctx.tool_chunks);
            let fields = tool_row_live_fields(message, ctx.locale, live);
            let prev_has = prev.live_tool_has_details.unwrap_or(false);
            // 结构未变：只改 status / one-line 文案，避免 ReplaceAll 抖高。
            if prev_has == fields.wants_details() {
                return Some(LiveBodyPlan {
                    message_id: id.to_string(),
                    patch: TuiBodyPatch::ToolRow {
                        status: fields.status,
                        one_line: fields.one_line,
                        detail: fields.detail,
                    },
                });
            }
            return Some(LiveBodyPlan {
                message_id: id.to_string(),
                patch: TuiBodyPatch::ReplaceAll {
                    chunks: next_chunks,
                },
            });
        }
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
        ctx.tool_chunks,
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
                ctx.tool_chunks,
            );
            LiveBodyPlan {
                message_id: message.id.clone(),
                patch: TuiBodyPatch::ReplaceAll { chunks },
            }
        })
        .collect()
}

fn plan_refresh_actions(
    messages: &[StoredMessage],
    live_id: Option<&str>,
    locale: Locale,
) -> Vec<TurnActionsPlan> {
    messages
        .iter()
        .enumerate()
        .filter(|(_, message)| live_id != Some(message.id.as_str()))
        .map(|(msg_idx, message)| TurnActionsPlan {
            message_id: message.id.clone(),
            html: turn_actions_bar_html(message, msg_idx, locale),
        })
        .collect()
}

fn actions_for_promote(
    messages: &[StoredMessage],
    promote_id: Option<&str>,
    locale: Locale,
) -> Vec<TurnActionsPlan> {
    let Some(pid) = promote_id else {
        return Vec::new();
    };
    messages
        .iter()
        .enumerate()
        .find(|(_, m)| m.id == pid)
        .map(|(msg_idx, message)| TurnActionsPlan {
            message_id: message.id.clone(),
            html: turn_actions_bar_html(message, msg_idx, locale),
        })
        .into_iter()
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
                ctx.tool_chunks,
            )
        })
    });
    let live_tool_has_details =
        live_tool_has_details_flag(messages, live_id.as_deref(), ctx.locale, ctx.tool_chunks);
    TuiMountState {
        session_id: ctx.session_id.to_string(),
        mounted_ids: messages.iter().map(|m| m.id.clone()).collect(),
        committed_key,
        live_id,
        live_body,
        live_tool_has_details,
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
    tool_chunks: &HashMap<String, String>,
) -> TuiSyncPlan {
    let Some(prev) = prev else {
        return full_rebuild_plan(
            messages,
            session_id,
            overlay,
            locale,
            apply_assistant_display_filters,
            tool_chunks,
        );
    };
    if must_full_rebuild(prev, messages, session_id) {
        return full_rebuild_plan(
            messages,
            session_id,
            overlay,
            locale,
            apply_assistant_display_filters,
            tool_chunks,
        );
    }

    let ctx = TuiRenderCtx {
        session_id,
        overlay,
        locale,
        apply_filters: apply_assistant_display_filters,
        tool_chunks,
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
            refresh_actions: Vec::new(),
            live,
        };
    }

    if same_turn_ids(prev, messages) && append_sections.is_empty() {
        let refresh_bodies =
            plan_refresh_bodies(messages, live_id.as_deref(), promote_id.as_deref(), &ctx);
        let mut refresh_actions = plan_refresh_actions(messages, live_id.as_deref(), locale);
        refresh_actions.extend(actions_for_promote(messages, promote_id.as_deref(), locale));
        return TuiSyncPlan {
            next,
            full_html: None,
            promote_id,
            append_sections: Vec::new(),
            refresh_bodies,
            refresh_actions,
            live,
        };
    }

    let refresh_actions = actions_for_promote(messages, promote_id.as_deref(), locale);
    TuiSyncPlan {
        next,
        full_html: None,
        promote_id,
        append_sections,
        refresh_bodies: Vec::new(),
        refresh_actions,
        live,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessageState;
    use crate::stream_text_overlay::StreamTextOverlay;
    use std::collections::HashMap;

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
            &HashMap::new(),
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
            &HashMap::new(),
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
            &HashMap::new(),
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
            &HashMap::new(),
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
        let plan1 = plan_tui_sync(
            None,
            &messages,
            "s1",
            None,
            Locale::ZhHans,
            false,
            &HashMap::new(),
        );
        let plan2 = plan_tui_sync(
            Some(&plan1.next),
            &messages,
            "s2",
            None,
            Locale::ZhHans,
            false,
            &HashMap::new(),
        );
        assert!(plan2.full_html.is_some());
    }

    #[test]
    fn finished_assistant_bold_becomes_strong() {
        let messages = vec![
            message("u1", "user", "你好"),
            message("a1", "assistant", "**原样**"),
        ];
        let output = build_tui_transcript_html(
            &messages,
            "s1",
            None,
            Locale::ZhHans,
            false,
            &HashMap::new(),
        );
        assert!(output.contains("data-tui-msg-id"), "got {output}");
        assert!(output.contains("chat-tui-turn--user"), "got {output}");
        assert!(output.contains("chat-tui-turn--assistant"), "got {output}");
        assert!(output.contains("chat-tui-role-label"), "got {output}");
        for section in output.split("<section class=\"chat-tui-turn") {
            if let Some(end) = section.find("</section>") {
                assert!(
                    !section[..end].contains("chat-tui-role-label"),
                    "role label must be outside section card, got {output}"
                );
            }
        }
        assert!(output.contains("chat-tui-turn-actions"), "got {output}");
        assert!(output.contains("data-tui-action=\"copy\""), "got {output}");
        assert!(!output.contains('❯'), "got {output}");
        assert!(
            output.contains("<strong>") || output.contains("<b>"),
            "got {output}"
        );
    }

    #[test]
    fn tool_turn_uses_tool_modifier_without_generic_role_word() {
        let mut tool = message("t1", "assistant", "ok");
        tool.is_tool = true;
        tool.tool_name = Some("read_file".to_string());
        let output =
            build_tui_transcript_html(&[tool], "s1", None, Locale::ZhHans, false, &HashMap::new());
        assert!(output.contains("chat-tui-turn--tool"), "got {output}");
        assert!(output.contains("chat-tui-tool-process"), "got {output}");
        assert!(output.contains("read_file"), "got {output}");
        assert!(!output.contains("工具:"), "got {output}");
    }

    #[test]
    fn live_tool_chunk_uses_tool_row_patch_not_replace_all() {
        let user = message("u1", "user", "hi");
        let mut tool = message("t1", "assistant", "");
        tool.is_tool = true;
        tool.tool_name = Some("read_file".to_string());
        tool.tool_call_id = Some("tc1".to_string());
        tool.state = Some(StoredMessageState::Loading);
        let messages = vec![user, tool];
        let mut chunks = HashMap::new();
        chunks.insert("tc1".to_string(), "part-a".to_string());
        let plan1 = plan_tui_sync(None, &messages, "s1", None, Locale::ZhHans, false, &chunks);
        assert!(plan1.full_html.is_some());
        assert_eq!(plan1.next.live_tool_has_details, Some(false));

        chunks.insert("tc1".to_string(), "part-a part-b".to_string());
        let plan2 = plan_tui_sync(
            Some(&plan1.next),
            &messages,
            "s1",
            None,
            Locale::ZhHans,
            false,
            &chunks,
        );
        assert!(plan2.full_html.is_none());
        let live = plan2.live.expect("tool live patch");
        assert_eq!(live.message_id, "t1");
        match live.patch {
            TuiBodyPatch::ToolRow {
                status,
                one_line,
                detail,
            } => {
                assert!(
                    status.contains("执行") || status.contains("running") || !status.is_empty()
                );
                assert!(one_line.contains("part-b"), "{one_line}");
                assert!(detail.is_none());
            }
            other => panic!("expected ToolRow, got {other:?}"),
        }
    }
}
