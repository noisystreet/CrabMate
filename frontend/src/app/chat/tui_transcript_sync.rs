//! TUI transcript：每回合独立 wrap（section + 操作条）；工具为一行摘要；流式只对 live 按块 patch（闭合冻结、活跃块行内增强）。

use std::collections::HashMap;

use crate::i18n::Locale;
use crate::markdown::plaintext_to_safe_html;
use crate::storage::{StoredMessage, StoredMessageState};
use crate::stream_text_overlay::{
    StreamTextOverlay, message_text_for_display_including_stream_overlay,
};

use super::tui_actions_bar::long_assistant_turn_class_suffix;
use super::tui_line_markdown::{
    TuiBodyChunks, TuiBodyPatch, open_active_block_class, parse_tui_body_chunks_with,
    plan_tui_body_patch, render_open_active_html,
};
use super::tui_tool_process::{tool_process_body_html, tool_row_live_fields};
use crate::visible_messages::tui_should_render_message;

/// 可挂载回合（跳过空助手壳；保留原始下标供操作条）。
fn mountable_turns<'a>(
    messages: &'a [StoredMessage],
    session_id: &str,
    overlay: Option<&StreamTextOverlay>,
) -> Vec<(usize, &'a StoredMessage)> {
    messages
        .iter()
        .enumerate()
        .filter(|(_, m)| tui_should_render_message(m, messages, session_id, overlay))
        .collect()
}

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
    /// Incremental 应用时读取（与 [`TuiBodyChunks::markdown_render`] 同源，不放在 patch 里重复）。
    pub markdown_render: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TurnActionsPlan {
    pub message_id: String,
    pub html: String,
}

fn message_finalize_open_block(message: &StoredMessage) -> bool {
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
pub(crate) fn committed_fingerprint(
    mountable: &[(usize, &StoredMessage)],
    live_id: Option<&str>,
) -> u64 {
    let mut fingerprint = mountable.len() as u64;
    for (_, message) in mountable {
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

fn file_ref_chip_html(token: &str) -> String {
    let display = crate::message_format::file_ref_display::file_ref_visible_label(token);
    let title_esc = plaintext_to_safe_html(token);
    let display_esc = plaintext_to_safe_html(display);
    format!("<span class=\"msg-file-ref\" title=\"{title_esc}\">{display_esc}</span>")
}

/// 将内联 HTML 插入首个正文容器内（优先 `<p>` 内），避免 chip 与块级段落上下叠成两行。
fn prepend_inline_html_to_first_tui_line(line_html: &mut String, prefix: &str) {
    const P_OPEN: &str = "<p>";
    if let Some(i) = line_html.find(P_OPEN) {
        line_html.insert_str(i + P_OPEN.len(), prefix);
        return;
    }
    if let Some(p0) = line_html.find("<p ") {
        if let Some(gt) = line_html[p0..].find('>') {
            line_html.insert_str(p0 + gt + 1, prefix);
            return;
        }
    }
    if let Some(i) = line_html.find('>') {
        line_html.insert_str(i + 1, prefix);
    } else {
        line_html.insert_str(0, prefix);
    }
}

/// skill chip + 任务正文：chip 必须与首行同块，不能先单独塞一个裸 span 再跟 `div.chat-tui-line`。
fn skill_slash_body_chunks(
    skill_id: &str,
    task: &str,
    finalize_open_block: bool,
    markdown_render: bool,
    locale: Locale,
) -> TuiBodyChunks {
    let prefix = crate::i18n::msg_skill_invoke_prefix(locale);
    let suffix = crate::i18n::msg_skill_invoke_suffix(locale);
    let id_esc = plaintext_to_safe_html(skill_id);
    let title_esc = plaintext_to_safe_html(&format!("/{skill_id}"));
    let chip = format!(
        "<span class=\"msg-skill-invoke\" title=\"{title_esc}\">{prefix} <span class=\"msg-skill-invoke-id\">{id_esc}</span> {suffix}</span>"
    );
    if task.is_empty() {
        return TuiBodyChunks {
            closed: vec![format!(
                "<div class=\"chat-tui-line chat-tui-line--block\">{chip}</div>"
            )],
            open_plain: None,
            markdown_render,
        };
    }
    let mut task_chunks = user_text_body_chunks(task, finalize_open_block, markdown_render);
    let chip_prefix = format!("{chip} ");
    if let Some(first) = task_chunks.closed.first_mut() {
        prepend_inline_html_to_first_tui_line(first, &chip_prefix);
        return task_chunks;
    }
    if let Some(plain) = task_chunks.open_plain.take() {
        let class = open_active_block_class(&plain, markdown_render);
        let body = render_open_active_html(&plain, markdown_render);
        task_chunks
            .closed
            .push(format!("<div class=\"{class}\">{chip_prefix}{body}</div>"));
        return task_chunks;
    }
    TuiBodyChunks {
        closed: vec![format!(
            "<div class=\"chat-tui-line chat-tui-line--block\">{chip}</div>"
        )],
        open_plain: None,
        markdown_render,
    }
}

fn user_text_body_chunks(
    text: &str,
    finalize_open_block: bool,
    markdown_render: bool,
) -> TuiBodyChunks {
    use crate::message_format::file_ref_display::{UserTextSeg, split_user_file_ref_segs};
    let segs = split_user_file_ref_segs(text);
    if segs.iter().all(|s| matches!(s, UserTextSeg::Plain(_))) {
        return parse_tui_body_chunks_with(text, finalize_open_block, markdown_render);
    }

    // 占位符整段解析后再换回 chip，避免裸 span 落在 `chat-tui-line` 外造成额外换行。
    const MARK_L: &str = "\u{2060}⟦CMFR";
    const MARK_R: &str = "⟧\u{2060}";
    let mut rebuilt = String::with_capacity(text.len());
    let mut chips: Vec<String> = Vec::new();
    for seg in segs {
        match seg {
            UserTextSeg::Plain(p) => rebuilt.push_str(&p),
            UserTextSeg::FileRef(tok) => {
                let i = chips.len();
                chips.push(file_ref_chip_html(&tok));
                rebuilt.push_str(MARK_L);
                rebuilt.push_str(&i.to_string());
                rebuilt.push_str(MARK_R);
            }
        }
    }

    let mut chunks = parse_tui_body_chunks_with(&rebuilt, finalize_open_block, markdown_render);
    let replace_marks = |s: &mut String| {
        for (i, chip) in chips.iter().enumerate() {
            let mark = format!("{MARK_L}{i}{MARK_R}");
            if s.contains(&mark) {
                *s = s.replace(&mark, chip);
                continue;
            }
            let esc = plaintext_to_safe_html(&mark);
            if esc != mark {
                *s = s.replace(&esc, chip);
            }
        }
    };
    for c in &mut chunks.closed {
        replace_marks(c);
    }
    // open_plain 是源文本不能嵌 HTML：有引用时折成闭合行并替换占位符。
    if let Some(plain) = chunks.open_plain.take() {
        let class = open_active_block_class(&plain, markdown_render);
        let mut body = render_open_active_html(&plain, markdown_render);
        replace_marks(&mut body);
        chunks
            .closed
            .push(format!("<div class=\"{class}\">{body}</div>"));
    }
    chunks
}

fn message_body_chunks(message: &StoredMessage, ctx: &TuiRenderCtx<'_>) -> TuiBodyChunks {
    if message.is_tool {
        let live = tool_live_overlay(message, ctx.tool_chunks);
        return TuiBodyChunks {
            closed: vec![tool_process_body_html(message, ctx.locale, live)],
            open_plain: None,
            // 工具 HTML 不走 MD，仍记录全局开关以免与 Incremental 前缀比较漂移。
            markdown_render: ctx.markdown_render,
        };
    }
    let text = message_display_text(
        message,
        ctx.session_id,
        ctx.overlay,
        ctx.locale,
        ctx.apply_filters,
    );
    if message.role == "user"
        && let Some((skill_id, task)) = crate::message_format::parse_user_skill_slash(&text)
    {
        return skill_slash_body_chunks(
            &skill_id,
            &task,
            message_finalize_open_block(message),
            ctx.markdown_render,
            ctx.locale,
        );
    }
    if message.role == "user" {
        return user_text_body_chunks(
            &text,
            message_finalize_open_block(message),
            ctx.markdown_render,
        );
    }
    parse_tui_body_chunks_with(
        &text,
        message_finalize_open_block(message),
        ctx.markdown_render,
    )
}

struct TurnSectionArgs<'a> {
    message: &'a StoredMessage,
    msg_idx: usize,
    is_live: bool,
    ctx: &'a TuiRenderCtx<'a>,
}

fn turn_section_html(args: TurnSectionArgs<'_>) -> String {
    let TurnSectionArgs {
        message,
        msg_idx,
        is_live,
        ctx,
    } = args;
    let locale = ctx.locale;
    let role = tui_role_label(message, locale);
    let body = message_body_chunks(message, ctx).to_inner_html();
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
    let long_class = long_assistant_turn_class_suffix(message);
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
        "<section class=\"chat-tui-turn {role_class}{live_class}{loading_class}{long_class}\" data-tui-msg-id=\"{id_esc}\"{live_attr}>\
         <div class=\"chat-tui-body\">{body}</div>\
         </section>"
    );
    let wrap_align = if role_class == "chat-tui-turn--user" {
        " chat-tui-turn-wrap--user"
    } else {
        ""
    };
    // 操作改为右键 / 长按菜单；idx 供菜单分发 regen/branch。
    format!(
        "<div class=\"chat-tui-turn-wrap{wrap_align}\" data-tui-wrap-id=\"{id_esc}\" \
         data-tui-msg-idx=\"{msg_idx}\">{role_block}{section}</div>"
    )
}

#[must_use]
pub(crate) fn build_tui_transcript_html(
    messages: &[StoredMessage],
    session_id: &str,
    overlay: Option<&StreamTextOverlay>,
    locale: Locale,
    apply_assistant_display_filters: bool,
    markdown_render: bool,
    tool_chunks: &HashMap<String, String>,
) -> String {
    let turns = mountable_turns(messages, session_id, overlay);
    if turns.is_empty() {
        return empty_transcript_html(locale);
    }
    let ctx = TuiRenderCtx {
        session_id,
        overlay,
        locale,
        apply_filters: apply_assistant_display_filters,
        markdown_render,
        tool_chunks,
    };
    let live_id = live_message_id(messages, overlay);
    let mut html = String::new();
    for &(msg_idx, message) in &turns {
        let is_live = live_id.as_deref() == Some(message.id.as_str());
        html.push_str(&turn_section_html(TurnSectionArgs {
            message,
            msg_idx,
            is_live,
            ctx: &ctx,
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

fn ids_prefix(mounted: &[String], turns: &[(usize, &StoredMessage)]) -> bool {
    if mounted.len() > turns.len() {
        return false;
    }
    mounted
        .iter()
        .zip(turns.iter())
        .all(|(id, (_, message))| id == &message.id)
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
    markdown_render: bool,
    tool_chunks: &HashMap<String, String>,
) -> TuiSyncPlan {
    let turns = mountable_turns(messages, session_id, overlay);
    let live_id = live_message_id(messages, overlay);
    let committed_key = committed_fingerprint(&turns, live_id.as_deref());
    let mounted_ids: Vec<String> = turns.iter().map(|(_, m)| m.id.clone()).collect();
    let ctx = TuiRenderCtx {
        session_id,
        overlay,
        locale,
        apply_filters: apply_assistant_display_filters,
        markdown_render,
        tool_chunks,
    };
    let live_body = live_id.as_ref().and_then(|id| {
        // 仅当 live 已可挂载时缓存 body（空壳未挂载则无 live_body）
        turns
            .iter()
            .find(|(_, m)| m.id == *id)
            .map(|(_, m)| message_body_chunks(m, &ctx))
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
            markdown_render,
            tool_chunks,
        )),
        promote_id: None,
        append_sections: Vec::new(),
        refresh_bodies: Vec::new(),
        refresh_actions: Vec::new(),
        live: None,
    }
}

fn must_full_rebuild(
    prev: &TuiMountState,
    turns: &[(usize, &StoredMessage)],
    session_id: &str,
) -> bool {
    prev.session_id != session_id
        || (turns.is_empty() && !prev.mounted_ids.is_empty())
        || !ids_prefix(&prev.mounted_ids, turns)
        || prev.mounted_ids.len() > turns.len()
}

fn same_turn_ids(prev: &TuiMountState, turns: &[(usize, &StoredMessage)]) -> bool {
    prev.mounted_ids.len() == turns.len()
        && prev
            .mounted_ids
            .iter()
            .zip(turns.iter())
            .all(|(id, (_, message))| id == &message.id)
}

struct TuiRenderCtx<'a> {
    session_id: &'a str,
    overlay: Option<&'a StreamTextOverlay>,
    locale: Locale,
    apply_filters: bool,
    markdown_render: bool,
    tool_chunks: &'a HashMap<String, String>,
}

fn append_new_turn_sections(
    prev: &TuiMountState,
    turns: &[(usize, &StoredMessage)],
    live_id: Option<&str>,
    ctx: &TuiRenderCtx<'_>,
) -> Vec<String> {
    turns
        .iter()
        .skip(prev.mounted_ids.len())
        .map(|&(msg_idx, message)| {
            turn_section_html(TurnSectionArgs {
                message,
                msg_idx,
                is_live: live_id == Some(message.id.as_str()),
                ctx,
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

fn live_body_plan(message_id: &str, markdown_render: bool, patch: TuiBodyPatch) -> LiveBodyPlan {
    LiveBodyPlan {
        message_id: message_id.to_string(),
        patch,
        markdown_render,
    }
}

fn plan_live_tool_patch(
    prev: &TuiMountState,
    message: &StoredMessage,
    id: &str,
    next_chunks: TuiBodyChunks,
    ctx: &TuiRenderCtx<'_>,
) -> LiveBodyPlan {
    let live = tool_live_overlay(message, ctx.tool_chunks);
    let fields = tool_row_live_fields(message, ctx.locale, live);
    let prev_has = prev.live_tool_has_details.unwrap_or(false);
    // 结构未变：只改 status / one-line 文案，避免 ReplaceAll 抖高。
    let md = next_chunks.markdown_render;
    if prev_has == fields.wants_details() {
        return live_body_plan(
            id,
            md,
            TuiBodyPatch::ToolRow {
                status: fields.status,
                status_label: fields.status_label,
                one_line: fields.one_line,
                detail: fields.detail,
            },
        );
    }
    live_body_plan(
        id,
        md,
        TuiBodyPatch::ReplaceAll {
            chunks: next_chunks,
        },
    )
}

fn plan_live_text_patch(
    prev: &TuiMountState,
    id: &str,
    next_chunks: TuiBodyChunks,
) -> LiveBodyPlan {
    let prev_chunks = prev
        .live_body
        .as_ref()
        .filter(|_| prev.live_id.as_deref() == Some(id));
    let md = next_chunks.markdown_render;
    let patch = plan_tui_body_patch(prev_chunks, &next_chunks);
    live_body_plan(id, md, patch)
}

fn plan_promote_body_patch(
    prev: &TuiMountState,
    messages: &[StoredMessage],
    promote_id: &str,
    ctx: &TuiRenderCtx<'_>,
) -> Option<LiveBodyPlan> {
    let message = messages.iter().find(|m| m.id == promote_id)?;
    let chunks = message_body_chunks(message, ctx);
    // 结束回合时用增量收口末块，避免整 body ReplaceAll 抖动。
    let prev_chunks = prev
        .live_body
        .as_ref()
        .filter(|_| prev.live_id.as_deref() == Some(promote_id));
    let md = chunks.markdown_render;
    let patch = plan_tui_body_patch(prev_chunks, &chunks);
    Some(live_body_plan(promote_id, md, patch))
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
        let next_chunks = message_body_chunks(message, ctx);
        if message.is_tool && prev.live_id.as_deref() == Some(id) {
            return Some(plan_live_tool_patch(prev, message, id, next_chunks, ctx));
        }
        return Some(plan_live_text_patch(prev, id, next_chunks));
    }
    plan_promote_body_patch(prev, messages, promote_id?, ctx)
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
            let chunks = message_body_chunks(message, ctx);
            let md = chunks.markdown_render;
            live_body_plan(&message.id, md, TuiBodyPatch::ReplaceAll { chunks })
        })
        .collect()
}

fn plan_refresh_actions(
    _messages: &[StoredMessage],
    _live_id: Option<&str>,
    _locale: Locale,
) -> Vec<TurnActionsPlan> {
    // 操作已迁至右键/长按菜单，不再刷新下方操作条 DOM。
    Vec::new()
}

fn actions_for_promote(
    _messages: &[StoredMessage],
    _promote_id: Option<&str>,
    _locale: Locale,
) -> Vec<TurnActionsPlan> {
    Vec::new()
}

fn next_mount_state(
    turns: &[(usize, &StoredMessage)],
    messages: &[StoredMessage],
    live_id: Option<String>,
    committed_key: u64,
    ctx: &TuiRenderCtx<'_>,
) -> TuiMountState {
    let live_body = live_id.as_ref().and_then(|id| {
        turns
            .iter()
            .find(|(_, m)| m.id == *id)
            .map(|(_, m)| message_body_chunks(m, ctx))
    });
    let live_tool_has_details =
        live_tool_has_details_flag(messages, live_id.as_deref(), ctx.locale, ctx.tool_chunks);
    TuiMountState {
        session_id: ctx.session_id.to_string(),
        mounted_ids: turns.iter().map(|(_, m)| m.id.clone()).collect(),
        committed_key,
        live_id,
        live_body,
        live_tool_has_details,
    }
}

/// [`plan_tui_sync`] 入参袋（避免形参超 frontend clippy 上限）。
pub(crate) struct PlanTuiSyncArgs<'a> {
    pub prev: Option<&'a TuiMountState>,
    pub messages: &'a [StoredMessage],
    pub session_id: &'a str,
    pub overlay: Option<&'a StreamTextOverlay>,
    pub locale: Locale,
    pub apply_assistant_display_filters: bool,
    pub markdown_render: bool,
    pub tool_chunks: &'a HashMap<String, String>,
}

/// 规划 transcript DOM 更新。
#[must_use]
pub(crate) fn plan_tui_sync(args: PlanTuiSyncArgs<'_>) -> TuiSyncPlan {
    let PlanTuiSyncArgs {
        prev,
        messages,
        session_id,
        overlay,
        locale,
        apply_assistant_display_filters,
        markdown_render,
        tool_chunks,
    } = args;
    let turns = mountable_turns(messages, session_id, overlay);
    let Some(prev) = prev else {
        return full_rebuild_plan(
            messages,
            session_id,
            overlay,
            locale,
            apply_assistant_display_filters,
            markdown_render,
            tool_chunks,
        );
    };
    if must_full_rebuild(prev, &turns, session_id) {
        return full_rebuild_plan(
            messages,
            session_id,
            overlay,
            locale,
            apply_assistant_display_filters,
            markdown_render,
            tool_chunks,
        );
    }

    let ctx = TuiRenderCtx {
        session_id,
        overlay,
        locale,
        apply_filters: apply_assistant_display_filters,
        markdown_render,
        tool_chunks,
    };
    let live_id = live_message_id(messages, overlay);
    let committed_key = committed_fingerprint(&turns, live_id.as_deref());
    let append_sections = append_new_turn_sections(prev, &turns, live_id.as_deref(), &ctx);
    let promote_id = promote_id_from(prev, live_id.as_deref());
    let live = plan_live_patch(
        prev,
        messages,
        live_id.as_deref(),
        promote_id.as_deref(),
        &ctx,
    );
    let next = next_mount_state(&turns, messages, live_id.clone(), committed_key, &ctx);

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

    if same_turn_ids(prev, &turns) && append_sections.is_empty() {
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
#[path = "tui_transcript_sync_tests.rs"]
mod tests;
