//! 单条聊天消息行的根视图 [`chat_message_row`]。

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;

use crate::i18n::Locale;
use crate::message_format::stored_message_is_staged_planner_round;
use crate::message_format::stored_tool_message_detail_text;
use crate::session_ops::format_msg_time_label;
use crate::storage::ChatSession;

use super::super::message_row_actions::MessageRowActionSignals;
use super::super::message_row_user_layout::{
    chat_row_wrap_and_user_styles, tool_bubble_detail_and_jump_uid,
};
use super::ChatMessageRowSignals;
use super::helpers::{
    build_subgoal_exec_banner_icon_key, build_subgoal_exec_banner_text,
    extract_hierarchical_goal_target, extract_hierarchical_metrics,
    extract_hierarchical_phase_chip, live_tool_message_compact_text, live_tool_message_detail_text,
    message_row_shell_class, tool_detail_drawer_body, tool_drawer_has_visible_body,
};
use super::row_extras::{
    BubbleClassLiveCtx, SubgoalBannerReactiveCtx, arc_actions_bar_visible,
    arc_retry_visible_for_message, bubble_css_classes_live, message_row_inline_copy_button,
    subgoal_exec_banner_reactive_view, typing_dots_tail_assistant_row,
};
use super::views::{
    ChatMessageRowBodyCoreParams, MessageActionsBarParams, build_message_actions_bar,
    chat_message_row_body_core, chat_message_row_meta_view,
};

fn terminal_session_drawer_classes(is_terminal_tool: bool) -> (&'static str, &'static str) {
    if is_terminal_tool {
        (
            "msg-tool-drawer msg-tool-drawer-below-card msg-tool-drawer-terminal",
            "msg-tool-drawer-pre msg-tool-drawer-pre-terminal",
        )
    } else {
        (
            "msg-tool-drawer msg-tool-drawer-below-card",
            "msg-tool-drawer-pre",
        )
    }
}

fn subgoal_metrics_line_view(line: Option<&String>) -> Option<AnyView> {
    line.map(|line| {
        let line = line.clone();
        view! { <div class="msg-subgoal-metrics-line">{line}</div> }.into_any()
    })
}

#[derive(Clone, Copy)]
struct ToolReasoningDrawerWire {
    is_tool_bubble: bool,
    tool_detail_expanded_ids: RwSignal<HashSet<String>>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    mid_drawer_sv: StoredValue<String>,
    locale: RwSignal<Locale>,
    is_terminal_tool: bool,
    drawer_panel_class: &'static str,
    drawer_pre_class: &'static str,
    tool_output_chunks: RwSignal<HashMap<String, String>>,
}

fn tool_reasoning_drawer_below_card(w: ToolReasoningDrawerWire) -> impl IntoView {
    let ToolReasoningDrawerWire {
        is_tool_bubble,
        tool_detail_expanded_ids,
        sessions,
        active_id,
        mid_drawer_sv,
        locale,
        is_terminal_tool,
        drawer_panel_class,
        drawer_pre_class,
        tool_output_chunks,
    } = w;
    view! {
        <Show
            when=move || {
                if !is_tool_bubble
                    || !tool_detail_expanded_ids.with(|s| {
                        s.contains(mid_drawer_sv.get_value().as_str())
                    })
                {
                    return false;
                }
                tool_drawer_has_visible_body(
                    sessions,
                    active_id,
                    mid_drawer_sv.get_value().as_str(),
                    locale.get(),
                    is_terminal_tool,
                    tool_output_chunks,
                )
            }
        >
            <div class=drawer_panel_class>
                <pre class=drawer_pre_class>
                    {move || {
                        let mid = mid_drawer_sv.get_value();
                        let loc = locale.get();
                        let raw = live_tool_message_detail_text(
                            sessions,
                            active_id,
                            mid.as_str(),
                            loc,
                            tool_output_chunks,
                        );
                        let compact = live_tool_message_compact_text(
                            sessions,
                            active_id,
                            mid.as_str(),
                            loc,
                            tool_output_chunks,
                        );
                        tool_detail_drawer_body(compact.as_str(), &raw, is_terminal_tool)
                    }}
                </pre>
            </div>
        </Show>
    }
}

pub(crate) fn chat_message_row(s: ChatMessageRowSignals) -> impl IntoView {
    let ChatMessageRowSignals {
        msg_idx,
        m,
        chat,
        collapsed_long_assistant_ids,
        chat_find_query,
        chat_find_match_ids,
        chat_find_cursor,
        auto_scroll_chat,
        stream_turn_busy_ui,
        tail_loading_assistant_mid,
        stream_follow_up,
        status_err,
        locale,
        markdown_render,
        apply_assistant_display_filters,
        tool_detail_expanded_ids,
        row_state_map,
    } = s;
    let sessions = chat.sessions;
    let active_id = chat.active_id;
    let row_actions = MessageRowActionSignals {
        chat,
        stream_follow_up,
        status_err,
        locale,
    };
    let cls = message_row_shell_class(&m);
    let mid_highlight = m.id.clone();
    let time_str = format_msg_time_label(m.created_at).unwrap_or_default();
    let mid_retry = m.id.clone();
    let copy_id = m.id.clone();
    let user_retry_id = m.id.clone();
    let user_branch_id = m.id.clone();
    let is_user_plain = m.role == "user" && !m.is_tool;
    let is_tool_bubble = m.is_tool;
    let (wrap_class, user_row_outer_style, user_row_stack_style, user_row_bubble_style) =
        chat_row_wrap_and_user_styles(m.role.as_str(), is_tool_bubble);
    let loc_ut = locale.get_untracked();
    let (tool_detail_text, jump_uid) = tool_bubble_detail_and_jump_uid(
        is_tool_bubble,
        stored_tool_message_detail_text(&m, loc_ut),
        sessions,
        active_id,
        msg_idx,
    );
    let msg_core = chat_message_row_body_core(ChatMessageRowBodyCoreParams {
        m: m.clone(),
        sessions,
        active_id,
        stream_text_overlay: chat.stream_text_overlay,
        stream_overlay_display_mid: chat.stream_overlay_display_mid,
        collapsed_long_assistant_ids,
        locale,
        markdown_render,
        apply_assistant_display_filters,
        chat_find_query,
        is_tool_bubble,
        tool_detail_text,
        tool_reasoning_live: is_tool_bubble.then(|| (sessions, active_id, mid_highlight.clone())),
        tool_detail_expanded_ids,
        tool_mid: mid_highlight.clone(),
        jump_uid,
        auto_scroll_chat,
        tool_output_chunks: chat.tool_output_chunks,
    });
    let retry_visible_rc = arc_retry_visible_for_message(row_state_map, mid_highlight.clone());
    let actions_bar_visible_rc =
        arc_actions_bar_visible(is_tool_bubble, is_user_plain, retry_visible_rc.clone());
    let show_planner_round_badge = stored_message_is_staged_planner_round(&m);
    let subgoal_phase_chip = extract_hierarchical_phase_chip(&m, loc_ut);
    let subgoal_metrics_line = extract_hierarchical_metrics(&m, loc_ut);
    let subgoal_target_line = extract_hierarchical_goal_target(&m);
    let phase_for_run_owned = subgoal_phase_chip.as_ref().map(|(phase, _)| phase.clone());
    let phase_for_banner_sl = phase_for_run_owned.as_deref();
    let subgoal_exec_banner =
        build_subgoal_exec_banner_text(loc_ut, phase_for_banner_sl, subgoal_target_line.as_deref());
    let subgoal_exec_banner_icon_key =
        build_subgoal_exec_banner_icon_key(loc_ut, phase_for_banner_sl);
    let mid_dom = m.id.clone();
    let is_terminal_tool = m.tool_name.as_deref() == Some("terminal_session");
    let (drawer_panel_class, drawer_pre_class) = terminal_session_drawer_classes(is_terminal_tool);
    let mid_drawer_sv = StoredValue::new(mid_highlight.clone());
    view! {
        <div class=wrap_class style=user_row_outer_style>
            <div class="msg-stack" style=user_row_stack_style>
                {true.then(|| {
                    chat_message_row_meta_view(
                        locale,
                        show_planner_round_badge,
                        m.clone(),
                        time_str.clone(),
                    )
                })}
                <div
                    class=move || {
                        let ctx = BubbleClassLiveCtx {
                            cls,
                            is_tool_bubble,
                            row_state_map,
                            mid_for_row: mid_highlight.clone(),
                            chat_find_query,
                            chat_find_match_ids,
                            chat_find_cursor,
                        };
                        bubble_css_classes_live(&ctx)
                    }
                    id=format!("msg-{mid_dom}")
                    data-testid=move || {
                        if is_tool_bubble {
                            "chat-tool-card"
                        } else {
                            "chat-message-row"
                        }
                    }
                    style=user_row_bubble_style
                >
                    {(!is_tool_bubble).then(|| {
                        message_row_inline_copy_button(
                            locale,
                            apply_assistant_display_filters,
                            sessions,
                            active_id,
                            chat.stream_text_overlay,
                            copy_id.clone(),
                        )
                    })}
                    {subgoal_exec_banner_reactive_view(SubgoalBannerReactiveCtx {
                        locale,
                        sessions,
                        active_id,
                        mid_subgoal: mid_highlight.clone(),
                        phase_for_run_owned,
                        subgoal_exec_banner,
                        subgoal_exec_banner_icon_key,
                    })}
                    {subgoal_metrics_line_view(subgoal_metrics_line.as_ref())}
                    {msg_core}
                    {(!is_tool_bubble).then(|| {
                        typing_dots_tail_assistant_row(
                            tail_loading_assistant_mid,
                            mid_highlight.clone(),
                        )
                    })}
                </div>
                {tool_reasoning_drawer_below_card(ToolReasoningDrawerWire {
                    is_tool_bubble,
                    tool_detail_expanded_ids,
                    sessions,
                    active_id,
                    mid_drawer_sv,
                    locale,
                    is_terminal_tool,
                    drawer_panel_class,
                    drawer_pre_class,
                    tool_output_chunks: chat.tool_output_chunks,
                })}
                {build_message_actions_bar(MessageActionsBarParams {
                    actions_bar_visible: actions_bar_visible_rc,
                    is_user_plain,
                    retry_visible: retry_visible_rc,
                    msg_idx,
                    user_retry_id: user_retry_id.clone(),
                    user_branch_id: user_branch_id.clone(),
                    mid_retry: mid_retry.clone(),
                    row_actions,
                    stream_follow_up,
                    stream_turn_busy_ui,
                    locale,
                })}
            </div>
        </div>
    }
}
