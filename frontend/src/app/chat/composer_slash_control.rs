//! Web composer：控制面 `/` 命令（不经模型）。密钥从不入聊天记录。

use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{
    clear_client_llm_api_key_storage, client_llm_storage_has_api_key, fetch_skills, fetch_status,
    fetch_workspace, load_client_llm_text_fields_from_storage, persist_client_llm_to_storage,
    post_config_reload, post_workspace_set,
};
use crate::app::chat::handles::ComposerStreamShell;
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::session_ops::{export_session_json_for_id, export_session_markdown_for_id};

/// 是否为 Web 控制斜杠（应拦截，不发送给模型）。`/<skill-id>` 除外。
#[must_use]
pub(super) fn is_web_control_slash(trimmed: &str) -> bool {
    let s = trimmed.trim();
    if !s.starts_with('/') {
        return false;
    }
    let head = s[1..]
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        head.as_str(),
        "help"
            | "?"
            | "workspace"
            | "cd"
            | "agent"
            | "export"
            | "config"
            | "model"
            | "api-base"
            | "apibase"
            | "context"
            | "clear"
            | "api-key"
            | "apikey"
            | "skills"
            | "version"
    )
}

#[derive(Clone, Copy)]
pub(super) struct WebSlashControlCtx {
    pub chat: ChatSessionSignals,
    pub locale: RwSignal<Locale>,
    pub draft: RwSignal<String>,
    pub selected_agent_role: RwSignal<Option<String>>,
    pub agent_role_user_override: RwSignal<bool>,
    pub apply_assistant_display_filters: RwSignal<bool>,
}

/// 尝试处理控制斜杠：返回 `true` 表示已消费（勿走 LLM）。
pub(super) fn try_handle_web_control_slash(
    text: &str,
    shell: &ComposerStreamShell,
    ctx: WebSlashControlCtx,
) -> bool {
    if !is_web_control_slash(text) {
        return false;
    }
    let loc = ctx.locale.get_untracked();
    let parts: Vec<&str> = text.split_whitespace().collect();
    let head = parts
        .first()
        .map(|s| s.trim_start_matches('/').to_ascii_lowercase())
        .unwrap_or_default();
    let args: Vec<&str> = parts.into_iter().skip(1).collect();

    match head.as_str() {
        "help" | "?" => {
            set_ok(
                shell,
                "常用 /：/workspace · /agent · /model · /api-base · /export · /config · /context · /skills · /clear · /api-key（密钥不进对话）",
            );
        }
        "version" => {
            set_ok(
                shell,
                format!("CrabMate Web · {}", env!("CARGO_PKG_VERSION")),
            );
        }
        "clear" => {
            let id = ctx.chat.active_id.get_untracked();
            ctx.chat.update_sessions_composer(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == id) {
                    s.messages.clear();
                    s.draft.clear();
                    s.history_total = None;
                    s.history_window_start = None;
                    s.history_has_older = None;
                }
            });
            ctx.draft.set(String::new());
            set_ok(shell, "已清空当前会话消息。");
        }
        "workspace" | "cd" => handle_workspace(&args, shell, loc),
        "agent" => handle_agent(&args, shell, ctx),
        "export" => handle_export(&args, shell, ctx, loc),
        "config" => handle_config(&args, shell, loc),
        "model" => handle_model(&args, shell, loc),
        "api-base" | "apibase" => handle_api_base(&args, shell, loc),
        "context" => handle_context(shell, loc),
        "skills" => handle_skills(shell, loc),
        "api-key" | "apikey" => handle_api_key(&args, shell, loc),
        _ => set_ok(shell, format!("未知控制命令 /{head}；输入 /help")),
    }
    ctx.draft.set(String::new());
    true
}

fn set_ok(shell: &ComposerStreamShell, msg: impl Into<String>) {
    shell.stream.status_err.set(Some(msg.into()));
}

fn handle_workspace(args: &[&str], shell: &ComposerStreamShell, loc: Locale) {
    let status = shell.stream.status_err;
    if args.is_empty() {
        spawn_local(async move {
            match fetch_workspace(None, loc).await {
                Ok(w) => status.set(Some(format!("工作区: {}", w.path))),
                Err(e) => status.set(Some(format!("工作区查询失败: {e}"))),
            }
        });
        return;
    }
    let path = args.join(" ");
    let refresh = Arc::clone(&shell.refresh_workspace);
    spawn_local(async move {
        match post_workspace_set(Some(path), loc).await {
            Ok(p) => {
                refresh();
                status.set(Some(format!("工作区已切换: {p}")));
            }
            Err(e) => status.set(Some(format!("切换失败: {e}"))),
        }
    });
}

fn handle_agent(args: &[&str], shell: &ComposerStreamShell, ctx: WebSlashControlCtx) {
    let sub = args.first().copied().unwrap_or("list");
    match sub {
        "list" => {
            let status = shell.stream.status_err;
            let loc = ctx.locale.get_untracked();
            let cur = ctx.selected_agent_role.get_untracked();
            spawn_local(async move {
                match fetch_status(loc).await {
                    Ok(s) => {
                        let ids = if s.agent_role_ids.is_empty() {
                            "（未配置多角色；可用 default）".to_string()
                        } else {
                            s.agent_role_ids.join(", ")
                        };
                        let cur_disp = cur
                            .filter(|c| !c.trim().is_empty())
                            .or(s.default_agent_role_id)
                            .unwrap_or_else(|| "default".into());
                        status.set(Some(format!("角色: {ids} · 当前 {cur_disp}")));
                    }
                    Err(e) => status.set(Some(format!("获取角色失败: {e}"))),
                }
            });
        }
        "set" => {
            let id = args.get(1).copied().unwrap_or("").trim();
            if id.is_empty() {
                set_ok(shell, "用法: /agent set <id> | /agent set default");
                return;
            }
            if id.eq_ignore_ascii_case("default") {
                ctx.selected_agent_role.set(None);
                ctx.agent_role_user_override.set(true);
                set_ok(shell, "已切到 default（清除显式角色）");
            } else {
                ctx.selected_agent_role.set(Some(id.to_string()));
                ctx.agent_role_user_override.set(true);
                set_ok(shell, format!("已设角色: {id}"));
            }
        }
        _ if args.is_empty() => handle_agent(&["list"], shell, ctx),
        _ => set_ok(shell, "用法: /agent · /agent list · /agent set <id>"),
    }
}

fn handle_export(args: &[&str], shell: &ComposerStreamShell, ctx: WebSlashControlCtx, loc: Locale) {
    let kind = args.first().copied().unwrap_or("json").to_ascii_lowercase();
    let id = ctx.chat.active_id.get_untracked();
    let filters = ctx.apply_assistant_display_filters.get_untracked();
    match kind.as_str() {
        "json" | "" => {
            export_session_json_for_id(ctx.chat.sessions, &id, loc, filters);
            set_ok(shell, "已触发 JSON 导出下载");
        }
        "markdown" | "md" => {
            export_session_markdown_for_id(ctx.chat.sessions, &id, loc, filters);
            set_ok(shell, "已触发 Markdown 导出下载");
        }
        "both" => {
            export_session_json_for_id(ctx.chat.sessions, &id, loc, filters);
            export_session_markdown_for_id(ctx.chat.sessions, &id, loc, filters);
            set_ok(shell, "已触发 JSON + Markdown 导出下载");
        }
        _ => set_ok(shell, "用法: /export [json|markdown|both]"),
    }
}

fn handle_config(args: &[&str], shell: &ComposerStreamShell, loc: Locale) {
    let sub = args.first().copied().unwrap_or("");
    if sub.eq_ignore_ascii_case("reload") {
        let status = shell.stream.status_err;
        spawn_local(async move {
            match post_config_reload(loc).await {
                Ok(msg) => status.set(Some(msg)),
                Err(e) => status.set(Some(format!("热重载失败: {e}"))),
            }
        });
        return;
    }
    if !sub.is_empty() {
        set_ok(shell, "用法: /config · /config reload");
        return;
    }
    let status = shell.stream.status_err;
    spawn_local(async move {
        match fetch_status(loc).await {
            Ok(s) => status.set(Some(format!(
                "model={} · api_base={} · ctx_tokens={} · sqlite_active={}",
                s.model, s.api_base, s.llm_context_tokens, s.conversation_store_sqlite_active
            ))),
            Err(e) => status.set(Some(format!("/config 失败: {e}"))),
        }
    });
}

fn handle_model(args: &[&str], shell: &ComposerStreamShell, loc: Locale) {
    if args.is_empty() {
        let (base, model, temp, ctx_tok, think) = load_client_llm_text_fields_from_storage();
        let model_disp = if model.trim().is_empty() {
            "(server default)".to_string()
        } else {
            model
        };
        set_ok(
            shell,
            format!(
                "model={model_disp} · api_base={base} · temp={temp} · ctx={ctx_tok} · think={think}"
            ),
        );
        return;
    }
    if !args
        .first()
        .copied()
        .unwrap_or("")
        .eq_ignore_ascii_case("set")
    {
        set_ok(shell, "用法: /model · /model set <id>");
        return;
    }
    let name = args.iter().skip(1).copied().collect::<Vec<_>>().join(" ");
    let name = name.trim();
    if name.is_empty() {
        set_ok(shell, "用法: /model set <id>");
        return;
    }
    let (base, _old, temp, ctx_tok, think) = load_client_llm_text_fields_from_storage();
    match persist_client_llm_to_storage(&base, name, &temp, &ctx_tok, &think, None, loc) {
        Ok(()) => set_ok(shell, format!("已设 model={name}（已同步 user-data）")),
        Err(e) => set_ok(shell, format!("写入失败: {e}")),
    }
}

fn handle_api_base(args: &[&str], shell: &ComposerStreamShell, loc: Locale) {
    if args.is_empty() {
        let (base, model, ..) = load_client_llm_text_fields_from_storage();
        set_ok(shell, format!("api_base={base} · model={model}"));
        return;
    }
    if !args
        .first()
        .copied()
        .unwrap_or("")
        .eq_ignore_ascii_case("set")
    {
        set_ok(shell, "用法: /api-base · /api-base set <url>");
        return;
    }
    let url = args.iter().skip(1).copied().collect::<Vec<_>>().join(" ");
    let url = url.trim();
    if url.is_empty() {
        set_ok(shell, "用法: /api-base set <url>");
        return;
    }
    let (_old, model, temp, ctx_tok, think) = load_client_llm_text_fields_from_storage();
    match persist_client_llm_to_storage(url, &model, &temp, &ctx_tok, &think, None, loc) {
        Ok(()) => set_ok(shell, format!("已设 api_base={url}（已同步 user-data）")),
        Err(e) => set_ok(shell, format!("写入失败: {e}")),
    }
}

fn handle_context(shell: &ComposerStreamShell, loc: Locale) {
    let status = shell.stream.status_err;
    spawn_local(async move {
        match fetch_status(loc).await {
            Ok(s) => status.set(Some(format!(
                "llm_context_tokens={} · effective_char_budget={} · counting_model={}",
                s.llm_context_tokens,
                s.effective_context_char_budget,
                s.tiktoken_prompt_counting_model
            ))),
            Err(e) => status.set(Some(format!("/context 失败: {e}"))),
        }
    });
}

fn handle_skills(shell: &ComposerStreamShell, loc: Locale) {
    let status = shell.stream.status_err;
    spawn_local(async move {
        match fetch_skills(loc).await {
            Ok(d) if !d.enabled => {
                status.set(Some("skills 已关闭（skills_enabled=false）".into()));
            }
            Ok(d) if d.skills.is_empty() => {
                status.set(Some(format!("当前未发现 skills（dir={}）", d.skills_dir)));
            }
            Ok(d) => {
                let mut lines = vec![format!("skills {} 个：", d.skills.len())];
                for s in d.skills.iter().take(24) {
                    lines.push(format!("  /{} — {}", s.id, s.description));
                }
                status.set(Some(lines.join("\n")));
            }
            Err(e) => status.set(Some(format!("列出 skills 失败: {e}"))),
        }
    });
}

fn handle_api_key(args: &[&str], shell: &ComposerStreamShell, loc: Locale) {
    let sub = args.first().copied().unwrap_or("").to_ascii_lowercase();
    match sub.as_str() {
        "" | "help" | "?" => set_ok(
            shell,
            "用法: /api-key status · /api-key set <密钥> · /api-key clear（密钥不进对话记录）",
        ),
        "status" => {
            let has = client_llm_storage_has_api_key();
            set_ok(
                shell,
                if has {
                    "本机已设置 client_llm API 密钥（值已隐藏）"
                } else {
                    "本机尚未设置 client_llm API 密钥"
                },
            );
        }
        "clear" => match clear_client_llm_api_key_storage(loc) {
            Ok(()) => set_ok(shell, "已清除 client_llm API 密钥"),
            Err(e) => set_ok(shell, format!("清除失败: {e}")),
        },
        "set" => {
            let secret = args.iter().skip(1).copied().collect::<Vec<_>>().join(" ");
            let secret = secret.trim();
            if secret.is_empty() {
                set_ok(shell, "用法: /api-key set <密钥>");
                return;
            }
            let (base, model, temp, ctx_tok, think) = load_client_llm_text_fields_from_storage();
            match persist_client_llm_to_storage(
                &base,
                &model,
                &temp,
                &ctx_tok,
                &think,
                Some(secret),
                loc,
            ) {
                Ok(()) => set_ok(shell, "已写入 API 密钥（不进对话；已同步 user-data）"),
                Err(e) => set_ok(shell, format!("写入失败: {e}")),
            }
        }
        _ => set_ok(
            shell,
            "用法: /api-key status · /api-key set <密钥> · /api-key clear",
        ),
    }
}
