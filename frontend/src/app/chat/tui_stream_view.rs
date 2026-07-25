//! TUI 风格聊天视图：每回合独立 section；live 按行局部更新（Phase 2）。

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::scroll_follow::follow_after_content_paint;
use super::scroll_shell::ChatScrollShellSignals;
use super::tui_line_markdown::TuiBodyPatch;
use super::tui_transcript_sync::{TuiMountState, TuiSyncPlan, plan_tui_sync};
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;

fn ensure_plain_line(body: &web_sys::HtmlElement) -> Option<web_sys::HtmlElement> {
    if let Some(existing) = body
        .query_selector(".chat-tui-line--plain")
        .ok()
        .flatten()
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
    {
        return Some(existing);
    }
    let document = body.owner_document()?;
    let plain = document
        .create_element("div")
        .ok()?
        .dyn_into::<web_sys::HtmlElement>()
        .ok()?;
    plain.set_class_name("chat-tui-line chat-tui-line--plain");
    let _ = body.append_child(&plain);
    Some(plain)
}

fn remove_plain_line(body: &web_sys::HtmlElement) {
    if let Some(plain) = body.query_selector(".chat-tui-line--plain").ok().flatten() {
        plain.remove();
    }
}

fn apply_body_patch(body: &web_sys::HtmlElement, patch: TuiBodyPatch) -> bool {
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
                // 闭合行晋升：先去掉旧 plain，再 append 闭合块，再写新 plain。
                remove_plain_line(body);
                for chunk in &append_closed {
                    if body.insert_adjacent_html("beforeend", chunk).is_err() {
                        return false;
                    }
                }
            }
            match open_plain {
                Some(text) => {
                    let Some(plain) = ensure_plain_line(body) else {
                        return false;
                    };
                    plain.set_text_content(Some(&text));
                }
                None => remove_plain_line(body),
            }
            true
        }
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

fn apply_tui_sync_plan(transcript: &web_sys::HtmlElement, plan: &TuiSyncPlan) -> bool {
    if let Some(html) = &plan.full_html {
        transcript.set_inner_html(html);
        return true;
    }

    if let Some(promote_id) = &plan.promote_id
        && let Some(section) = find_turn_section(transcript, promote_id)
    {
        let _ = section.remove_attribute("data-tui-live");
    }

    if !plan.append_sections.is_empty() {
        // 去掉空会话占位。
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
        if !apply_body_patch(&body, live.patch.clone()) {
            return false;
        }
    }

    for refresh in &plan.refresh_bodies {
        let Some(body) = find_turn_body(transcript, &refresh.message_id) else {
            return false;
        };
        if !apply_body_patch(&body, refresh.patch.clone()) {
            return false;
        }
    }

    true
}

#[component]
pub(crate) fn ChatTuiStreamView(
    chat: ChatSessionSignals,
    locale: RwSignal<Locale>,
    apply_assistant_display_filters: RwSignal<bool>,
    scroll_shell: ChatScrollShellSignals,
) -> impl IntoView {
    let transcript_ref = NodeRef::<leptos::html::Div>::new();
    let mount_state = RwSignal::new(None::<TuiMountState>);

    Effect::new(move |_| {
        let _ = chat.stream_overlay_revision.get();
        let active_id = chat.active_id.get();
        let locale = locale.get();
        let apply_filters = apply_assistant_display_filters.get();
        let overlay = chat.stream_text_overlay.get();
        let prev = mount_state.get_untracked();
        let plan = chat.sessions.with(|sessions| {
            let session = sessions.iter().find(|session| session.id == active_id);
            match session {
                None => plan_tui_sync(prev.as_ref(), &[], &active_id, None, locale, apply_filters),
                Some(session) => plan_tui_sync(
                    prev.as_ref(),
                    &session.messages,
                    &session.id,
                    overlay.as_ref(),
                    locale,
                    apply_filters,
                ),
            }
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
                let session = sessions.iter().find(|session| session.id == active_id);
                match session {
                    None => plan_tui_sync(None, &[], &active_id, None, locale, apply_filters),
                    Some(session) => plan_tui_sync(
                        None,
                        &session.messages,
                        &session.id,
                        overlay.as_ref(),
                        locale,
                        apply_filters,
                    ),
                }
            });
            let _ = apply_tui_sync_plan(el, &forced);
            mount_state.set(Some(forced.next));
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
            />
        </div>
    }
}
