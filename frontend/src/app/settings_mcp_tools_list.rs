//! MCP 设置页：单服务器可用工具列表（名称下方、可折叠）。

use leptos::prelude::*;

use crate::api::user_data::{McpServerStatusEntryDto, McpServersStatusDto};
use crate::i18n::{self, Locale};

#[derive(Debug, Clone)]
pub(crate) struct McpToolDisplayRow {
    pub name: String,
    pub description: Option<String>,
    pub openai_name: Option<String>,
}

pub(crate) fn status_for_server<'a>(
    status: Option<&'a McpServersStatusDto>,
    server_id: &str,
) -> Option<&'a McpServerStatusEntryDto> {
    status.and_then(|s| s.servers.iter().find(|e| e.id == server_id))
}

pub(crate) fn tools_to_display(row: &McpServerStatusEntryDto) -> Vec<McpToolDisplayRow> {
    if !row.remote_tools.is_empty() {
        return row
            .remote_tools
            .iter()
            .map(|t| {
                let openai = row
                    .openai_tool_names
                    .iter()
                    .find(|n| n.ends_with(&format!("__{}", t.name)))
                    .cloned();
                McpToolDisplayRow {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    openai_name: openai,
                }
            })
            .collect();
    }
    row.openai_tool_names
        .iter()
        .map(|n| McpToolDisplayRow {
            name: label_from_openai_tool_name(n),
            description: None,
            openai_name: Some(n.clone()),
        })
        .collect()
}

const TOOLS_COLLAPSE_CHEVRON: &str = "\u{25B8}";

fn label_from_openai_tool_name(openai_name: &str) -> String {
    let rest = openai_name.strip_prefix("mcp__").unwrap_or(openai_name);
    rest.split_once("__")
        .map(|(_, remote)| remote.to_string())
        .unwrap_or_else(|| rest.to_string())
}

fn toggle_aria_label(
    loc: Locale,
    status: Option<&McpServersStatusDto>,
    server_id: &str,
    probing: bool,
) -> String {
    let base = i18n::settings_mcp_tools_toggle_label(
        loc,
        tool_count_for_toggle(status, server_id, probing),
    );
    let Some(row) = status_for_server(status, server_id) else {
        return base;
    };
    let slug = row.slug.trim();
    if slug.is_empty() {
        format!("{base} — {}", row.name)
    } else {
        format!("{base} — {} ({slug})", row.name)
    }
}

fn tool_count_for_toggle(
    status: Option<&McpServersStatusDto>,
    server_id: &str,
    probing: bool,
) -> Option<usize> {
    if probing {
        return None;
    }
    let row = status_for_server(status, server_id)?;
    if !row.enabled || !row.connected {
        return None;
    }
    let n = tools_to_display(row).len();
    Some(n)
}

#[component]
pub(crate) fn SettingsMcpServerToolsList(
    locale: RwSignal<Locale>,
    server_id: String,
    status: ReadSignal<Option<McpServersStatusDto>>,
    probing: ReadSignal<bool>,
    expanded: RwSignal<bool>,
) -> impl IntoView {
    let server_id_for_test = server_id.clone();
    let server_id_panel_id = server_id.clone();
    let server_id_panel_controls = server_id.clone();
    let server_id_toggle_aria = server_id.clone();
    let server_id_toggle_label = server_id.clone();
    let server_id_body = server_id;

    view! {
        <div
            class="settings-mcp-tools"
            data-testid=format!("mcp-server-tools-{}", server_id_for_test)
        >
            <button
                type="button"
                class="settings-mcp-tools-toggle"
                data-testid="settings-mcp-tools-toggle"
                aria-expanded=move || expanded.get()
                aria-controls=format!("mcp-tools-panel-{}", server_id_panel_controls)
                prop:aria-label=move || {
                    toggle_aria_label(
                        locale.get(),
                        status.get().as_ref(),
                        &server_id_toggle_aria,
                        probing.get(),
                    )
                }
                prop:title=move || {
                    i18n::settings_mcp_tools_expand_aria(locale.get(), expanded.get())
                }
                on:click=move |_| expanded.update(|open| *open = !*open)
            >
                <span class="settings-mcp-tools-chevron" aria-hidden="true">{TOOLS_COLLAPSE_CHEVRON}</span>
                <span class="settings-mcp-tools-toggle-label" aria-hidden="true">{move || {
                    let loc = locale.get();
                    let sid = server_id_toggle_label.clone();
                    i18n::settings_mcp_tools_toggle_label(
                        loc,
                        tool_count_for_toggle(status.get().as_ref(), &sid, probing.get()),
                    )
                }}</span>
            </button>
            <div
                id=format!("mcp-tools-panel-{}", server_id_panel_id)
                class="settings-mcp-tools-panel"
                hidden=move || !expanded.get()
            >
                {move || {
                    let sid = server_id_body.clone();
                    if probing.get() {
                        return view! {
                            <p class="settings-intro settings-mcp-tools-hint">
                                {i18n::settings_mcp_tools_probing(locale.get_untracked())}
                            </p>
                        }
                        .into_any();
                    }
                    let loc = locale.get_untracked();
                    let row = status_for_server(status.get().as_ref(), &sid).cloned();
                    match row {
                        Some(r) if !r.enabled => view! {
                            <p class="settings-intro settings-mcp-tools-hint">
                                {i18n::settings_mcp_tools_server_disabled(loc)}
                            </p>
                        }
                        .into_any(),
                        Some(r) if r.connected => {
                            let tools = tools_to_display(&r);
                            if tools.is_empty() {
                                return view! {
                                    <p class="settings-intro settings-mcp-tools-hint">
                                        {i18n::settings_mcp_tools_none(loc)}
                                    </p>
                                }
                                .into_any();
                            }
                            view! {
                                <div class="settings-mcp-tools-table" data-testid="mcp-tools-list">
                                    <div class="settings-mcp-tools-header">
                                        <span class="settings-mcp-tools-col-name">
                                            {i18n::settings_mcp_tools_col_name(loc)}
                                        </span>
                                        <span class="settings-mcp-tools-col-desc">
                                            {i18n::settings_mcp_tools_col_description(loc)}
                                        </span>
                                    </div>
                                    <ul class="settings-mcp-tools-list">
                                        <For
                                            each=move || tools.clone()
                                            key=|t| t.name.clone()
                                            children=move |tool: McpToolDisplayRow| {
                                                let name = tool.name.clone();
                                                let desc = tool
                                                    .description
                                                    .filter(|d| !d.trim().is_empty())
                                                    .unwrap_or_else(|| {
                                                        i18n::settings_mcp_tools_desc_empty(loc).to_string()
                                                    });
                                                let openai = tool.openai_name.clone();
                                                view! {
                                                    <li class="settings-mcp-tool-item">
                                                        <div class="settings-mcp-tool-col-name">
                                                            <span class="settings-mcp-tool-name">{name.clone()}</span>
                                                            {openai.filter(|o| label_from_openai_tool_name(o) != name).map(|o| view! {
                                                                <code class="settings-mcp-tool-openai">{o}</code>
                                                            })}
                                                        </div>
                                                        <span class="settings-mcp-tool-desc">{desc}</span>
                                                    </li>
                                                }
                                            }
                                        />
                                    </ul>
                                </div>
                            }
                            .into_any()
                        }
                        Some(r) => {
                            let msg = r
                                .last_error
                                .filter(|e| !e.trim().is_empty())
                                .unwrap_or_else(|| i18n::settings_mcp_disconnected(loc).to_string());
                            view! {
                                <p class="settings-intro settings-mcp-tools-error">{msg}</p>
                            }
                            .into_any()
                        }
                        None => view! {
                            <p class="settings-intro settings-mcp-tools-hint">
                                {i18n::settings_mcp_tools_probe_hint(loc)}
                            </p>
                        }
                        .into_any(),
                    }
                }}
            </div>
        </div>
    }
}
