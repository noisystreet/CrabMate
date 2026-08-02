//! TUI 风格聊天视图：每回合独立 wrap（section + 下方操作条）；live 按块局部更新（活跃块流式行内 HTML）。

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::scroll_follow::follow_after_content_paint;
use super::scroll_shell::ChatScrollShellSignals;
use super::tui_actions_bar::{TuiTurnActionHandlers, dispatch_tui_turn_action};
use super::tui_line_markdown::{
    TuiBodyPatch, open_active_block_class, open_block_is_fence_buffer, render_open_active_html,
};
use super::tui_transcript_sync::{PlanTuiSyncArgs, TuiMountState, TuiSyncPlan, plan_tui_sync};
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::storage::ChatSession;
use crate::stream_text_overlay::StreamTextOverlay;
use std::collections::HashMap;

struct PlanActiveSessionArgs<'a> {
    sessions: &'a [ChatSession],
    active_id: &'a str,
    prev: Option<&'a TuiMountState>,
    overlay: Option<&'a StreamTextOverlay>,
    locale: Locale,
    apply_filters: bool,
    markdown_render: bool,
    tool_chunks: &'a HashMap<String, String>,
}

fn plan_for_active_session(args: PlanActiveSessionArgs<'_>) -> TuiSyncPlan {
    let PlanActiveSessionArgs {
        sessions,
        active_id,
        prev,
        overlay,
        locale,
        apply_filters,
        markdown_render,
        tool_chunks,
    } = args;
    match sessions.iter().find(|session| session.id == active_id) {
        None => plan_tui_sync(PlanTuiSyncArgs {
            prev,
            messages: &[],
            session_id: active_id,
            overlay: None,
            locale,
            apply_assistant_display_filters: apply_filters,
            markdown_render,
            tool_chunks,
        }),
        Some(session) => plan_tui_sync(PlanTuiSyncArgs {
            prev,
            messages: &session.messages,
            session_id: &session.id,
            overlay,
            locale,
            apply_assistant_display_filters: apply_filters,
            markdown_render,
            tool_chunks,
        }),
    }
}

fn ensure_open_block(body: &web_sys::HtmlElement) -> Option<web_sys::HtmlElement> {
    if let Some(existing) = body
        .query_selector(".chat-tui-line--plain, .chat-tui-line--active")
        .ok()
        .flatten()
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
    {
        return Some(existing);
    }
    let document = body.owner_document()?;
    let block = document
        .create_element("div")
        .ok()?
        .dyn_into::<web_sys::HtmlElement>()
        .ok()?;
    block.set_class_name("chat-tui-line chat-tui-line--active");
    let _ = body.append_child(&block);
    Some(block)
}

fn remove_open_block(body: &web_sys::HtmlElement) {
    if let Some(block) = body
        .query_selector(".chat-tui-line--plain, .chat-tui-line--active")
        .ok()
        .flatten()
    {
        block.remove();
    }
}

fn apply_open_active_block(body: &web_sys::HtmlElement, text: &str, markdown_render: bool) -> bool {
    let Some(block) = ensure_open_block(body) else {
        return false;
    };
    block.set_class_name(open_active_block_class(text, markdown_render));
    // 未闭合围栏：textContent 避免半截 HTML；关 MD / 行内增强走统一入口。
    if markdown_render && open_block_is_fence_buffer(text) {
        block.set_text_content(Some(text));
    } else {
        block.set_inner_html(&render_open_active_html(text, markdown_render));
    }
    true
}

fn apply_tool_row_patch(
    body: &web_sys::HtmlElement,
    status: &str,
    one_line: &str,
    detail: Option<&str>,
) -> bool {
    let Some(status_el) = body
        .query_selector(".chat-tui-tool-status")
        .ok()
        .flatten()
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
    else {
        return false;
    };
    let Some(one_el) = body
        .query_selector(".chat-tui-tool-one-line")
        .ok()
        .flatten()
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
    else {
        return false;
    };
    status_el.set_text_content(Some(status));
    one_el.set_text_content(Some(one_line));
    match detail {
        Some(text) => {
            let Some(pre) = body
                .query_selector(".chat-tui-tool-detail-body")
                .ok()
                .flatten()
                .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
            else {
                // 需要 details 但 DOM 仍是无详情结构 → 交由全量重建
                return false;
            };
            pre.set_text_content(Some(text));
        }
        None => {
            if body
                .query_selector(".chat-tui-tool-details")
                .ok()
                .flatten()
                .is_some()
            {
                // DOM 仍有 details、计划已无 → 结构变化，重建
                return false;
            }
        }
    }
    true
}

fn apply_body_patch(
    body: &web_sys::HtmlElement,
    patch: TuiBodyPatch,
    markdown_render: bool,
) -> bool {
    match patch {
        TuiBodyPatch::ReplaceAll { chunks } => {
            body.set_inner_html(&chunks.to_inner_html());
            true
        }
        TuiBodyPatch::Incremental {
            append_closed,
            open_plain,
        } => {
            if !append_closed.is_empty() {
                remove_open_block(body);
                for chunk in &append_closed {
                    if body.insert_adjacent_html("beforeend", chunk).is_err() {
                        return false;
                    }
                }
            }
            match open_plain {
                Some(text) => apply_open_active_block(body, &text, markdown_render),
                None => {
                    remove_open_block(body);
                    true
                }
            }
        }
        TuiBodyPatch::ToolRow {
            status,
            one_line,
            detail,
        } => apply_tool_row_patch(body, &status, &one_line, detail.as_deref()),
    }
}

fn find_turn_section(
    transcript: &web_sys::HtmlElement,
    message_id: &str,
) -> Option<web_sys::HtmlElement> {
    let selector = format!("section.chat-tui-turn[data-tui-msg-id=\"{message_id}\"]");
    transcript
        .query_selector(&selector)
        .ok()
        .flatten()
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
}

fn find_turn_wrap(
    transcript: &web_sys::HtmlElement,
    message_id: &str,
) -> Option<web_sys::HtmlElement> {
    let selector = format!(".chat-tui-turn-wrap[data-tui-wrap-id=\"{message_id}\"]");
    transcript
        .query_selector(&selector)
        .ok()
        .flatten()
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
}

fn find_turn_body(
    transcript: &web_sys::HtmlElement,
    message_id: &str,
) -> Option<web_sys::HtmlElement> {
    let section = find_turn_section(transcript, message_id)?;
    section
        .query_selector(".chat-tui-body")
        .ok()
        .flatten()
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
}

fn apply_actions_html(transcript: &web_sys::HtmlElement, message_id: &str, html: &str) -> bool {
    let Some(wrap) = find_turn_wrap(transcript, message_id) else {
        return false;
    };
    if let Some(existing) = wrap.query_selector(".chat-tui-turn-actions").ok().flatten() {
        existing.remove();
    }
    if html.is_empty() {
        return true;
    }
    wrap.insert_adjacent_html("beforeend", html).is_ok()
}

fn apply_tui_sync_plan(transcript: &web_sys::HtmlElement, plan: &TuiSyncPlan) -> bool {
    if let Some(html) = &plan.full_html {
        transcript.set_inner_html(html);
        return true;
    }

    if let Some(promote_id) = &plan.promote_id
        && let Some(section) = find_turn_section(transcript, promote_id)
    {
        let _ = section.remove_attribute("data-tui-live");
        let _ = section.class_list().remove_1("chat-tui-turn--live");
        let _ = section.class_list().remove_1("is-loading");
    }

    if !plan.append_sections.is_empty() {
        if let Some(empty) = transcript.query_selector(".chat-tui-empty").ok().flatten() {
            empty.remove();
        }
        for section_html in &plan.append_sections {
            if transcript
                .insert_adjacent_html("beforeend", section_html)
                .is_err()
            {
                return false;
            }
        }
    }

    if let Some(live) = &plan.live {
        let Some(body) = find_turn_body(transcript, &live.message_id) else {
            return false;
        };
        if !apply_body_patch(&body, live.patch.clone(), live.markdown_render) {
            return false;
        }
    }

    for refresh in &plan.refresh_bodies {
        let Some(body) = find_turn_body(transcript, &refresh.message_id) else {
            return false;
        };
        if !apply_body_patch(&body, refresh.patch.clone(), refresh.markdown_render) {
            return false;
        }
    }

    for actions in &plan.refresh_actions {
        if !apply_actions_html(transcript, &actions.message_id, &actions.html) {
            return false;
        }
    }

    true
}

fn action_click_from_event(ev: &web_sys::MouseEvent) -> Option<(String, String, usize)> {
    let target = ev.target()?.dyn_into::<web_sys::Element>().ok()?;
    let btn = target
        .closest("[data-tui-action]")
        .ok()
        .flatten()?
        .dyn_into::<web_sys::HtmlElement>()
        .ok()?;
    let action = btn.get_attribute("data-tui-action")?;
    let message_id = btn.get_attribute("data-tui-msg-id")?;
    let msg_idx = btn
        .get_attribute("data-tui-msg-idx")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Some((action, message_id, msg_idx))
}

#[component]
pub(crate) fn ChatTuiStreamView(
    chat: ChatSessionSignals,
    locale: RwSignal<Locale>,
    apply_assistant_display_filters: RwSignal<bool>,
    markdown_render: RwSignal<bool>,
    scroll_shell: ChatScrollShellSignals,
    action_handlers: TuiTurnActionHandlers,
) -> impl IntoView {
    let transcript_ref = NodeRef::<leptos::html::Div>::new();
    let mount_state = RwSignal::new(None::<TuiMountState>);

    Effect::new(move |_| {
        let _ = chat.stream_overlay_revision.get();
        let tool_chunks = chat.tool_output_chunks.get();
        let active_id = chat.active_id.get();
        let locale = locale.get();
        let apply_filters = apply_assistant_display_filters.get();
        let md_on = markdown_render.get();
        let overlay = chat.stream_text_overlay.get();
        let prev = mount_state.get_untracked();
        let plan = chat.sessions.with(|sessions| {
            plan_for_active_session(PlanActiveSessionArgs {
                sessions,
                active_id: &active_id,
                prev: prev.as_ref(),
                overlay: overlay.as_ref(),
                locale,
                apply_filters,
                markdown_render: md_on,
                tool_chunks: &tool_chunks,
            })
        });

        let Some(node) = transcript_ref.get() else {
            return;
        };
        let Some(el) = node.dyn_ref::<web_sys::HtmlElement>() else {
            return;
        };

        let applied = apply_tui_sync_plan(el, &plan);
        if applied {
            mount_state.set(Some(plan.next));
        } else {
            let forced = chat.sessions.with(|sessions| {
                plan_for_active_session(PlanActiveSessionArgs {
                    sessions,
                    active_id: &active_id,
                    prev: None,
                    overlay: overlay.as_ref(),
                    locale,
                    apply_filters,
                    markdown_render: md_on,
                    tool_chunks: &tool_chunks,
                })
            });
            if apply_tui_sync_plan(el, &forced) {
                mount_state.set(Some(forced.next));
            } else {
                web_sys::console::warn_1(
                    &"chat-tui: forced rebuild failed; keeping previous mount_state".into(),
                );
            }
        }
        follow_after_content_paint(scroll_shell);
    });

    view! {
        <div
            class="messages-inner chat-tui-inner"
            data-testid="chat-tui-stream-view"
        >
            <div
                class="chat-tui-transcript"
                data-testid="chat-tui-transcript"
                node_ref=transcript_ref
                aria-live="polite"
                aria-atomic="false"
                on:click=move |ev| {
                    let Some((action, message_id, msg_idx)) = action_click_from_event(&ev) else {
                        return;
                    };
                    let _ = dispatch_tui_turn_action(action_handlers, &action, &message_id, msg_idx);
                }
            />
        </div>
    }
}
