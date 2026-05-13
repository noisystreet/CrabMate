//! TUI 专用斜杠：**`/conv`**、**`/branch`**（依赖 SQLite 会话库）；在通用 REPL 斜杠之前处理。

use std::sync::{Arc, Mutex};

use crate::config::SharedAgentConfig;
use crate::process_handles::ProcessHandles;
use crate::runtime::workspace_session;
use crate::tool_stats::ToolOutcomeRecorder;
use crate::types::Message;

use super::sqlite_session::TuiSqliteSessionState;
use super::{TuiAfterChatRoundRefresh, TuiModel, tui_refresh_after_chat_round};

pub(super) struct TuiSqliteSlashEnv<'a> {
    pub(super) cfg_holder: &'a SharedAgentConfig,
    pub(super) model: &'a Arc<Mutex<TuiModel>>,
    pub(super) work_dir: &'a std::path::Path,
    pub(super) tool_count: usize,
    pub(super) cli_no_stream: bool,
    pub(super) process_handles: &'a Arc<ProcessHandles>,
}

fn push_block(model: &Arc<Mutex<TuiModel>>, lines: &[String]) {
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    g.transcript.push_str("\n[/conv]\n");
    for ln in lines {
        g.transcript.push_str(ln);
        g.transcript.push('\n');
    }
    g.chat_snap_bottom_next_draw = true;
}

async fn tui_sqlite_slash_refresh_ui(
    env: &TuiSqliteSlashEnv<'_>,
    messages: &[Message],
    agent_role_owned: &Option<String>,
) {
    tui_refresh_after_chat_round(TuiAfterChatRoundRefresh {
        model: env.model,
        cfg_holder: env.cfg_holder,
        work_dir: env.work_dir,
        agent_role_owned,
        messages,
        tool_count: env.tool_count,
        cli_no_stream: env.cli_no_stream,
        sqlite_persist: None,
        process_handles: env.process_handles,
    })
    .await;
}

async fn tui_sqlite_no_db_conv_or_branch(
    trimmed: &str,
    env: &TuiSqliteSlashEnv<'_>,
    agent_role_owned: &Option<String>,
) -> Result<bool, Box<dyn std::error::Error>> {
    if !trimmed.starts_with("/conv") && !trimmed.starts_with("/branch") {
        return Ok(false);
    }
    push_block(
        env.model,
        &["未启用 SQLite 会话库。请在配置中设置非空 conversation_store_sqlite_path（与 Web serve 同源）。"
            .to_string()],
    );
    let chips = super::sidebar_text::tui_status_chips_line(env.cfg_holder, agent_role_owned).await;
    let mut g = env.model.lock().unwrap_or_else(|e| e.into_inner());
    g.status = format!("{} · /conv /branch 需要会话 SQLite", chips);
    Ok(true)
}

pub(super) async fn tui_try_consume_sqlite_slash(
    trimmed: &str,
    sqlite_slot: &mut Option<&mut TuiSqliteSessionState>,
    messages: &mut Vec<Message>,
    agent_role_owned: &mut Option<String>,
    env: &TuiSqliteSlashEnv<'_>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(sess) = sqlite_slot.as_mut() else {
        return tui_sqlite_no_db_conv_or_branch(trimmed, env, agent_role_owned).await;
    };

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let cmd = parts.first().copied().unwrap_or("");

    if cmd == "/branch" {
        let ord_s = parts.get(1).copied().unwrap_or("");
        let ord = match ord_s.parse::<usize>() {
            Ok(v) => v,
            Err(_) => {
                push_block(
                    env.model,
                    &[
                        "用法: /branch <before_user_ordinal>".into(),
                        "ordinal 为 0-based，语义与 Web POST /chat/branch 一致。".into(),
                    ],
                );
                return Ok(true);
            }
        };
        match sess.branch_before_user_ordinal(ord, messages, agent_role_owned) {
            Ok(()) => {
                push_block(
                    env.model,
                    &[format!(
                        "已分支：截断到第 {ord} 条用户消息之前（revision 已递增）。"
                    )],
                );
                tui_sqlite_slash_refresh_ui(env, messages.as_slice(), agent_role_owned).await;
            }
            Err(e) => {
                push_block(env.model, &[format!("分支失败: {e}")]);
                let chips =
                    super::sidebar_text::tui_status_chips_line(env.cfg_holder, agent_role_owned)
                        .await;
                let mut g = env.model.lock().unwrap_or_else(|e| e.into_inner());
                g.status = format!("{} · {e}", chips);
            }
        }
        return Ok(true);
    }

    if cmd != "/conv" {
        return Ok(false);
    }

    let sub = parts.get(1).copied().unwrap_or("help");
    match sub {
        "help" | "?" => {
            push_block(
                env.model,
                &[
                    "/conv list — 列出最近会话 id".into(),
                    "/conv open <id> — 切换会话".into(),
                    "/conv open last — 打开最近更新的会话".into(),
                    "/conv new — 新建会话（仅 system 引导）".into(),
                    "/branch <n> — 截断到用户 ordinal n 之前（Web 同源）".into(),
                    "环境变量 CM_TUI_CONVERSATION_ID 可指定启动时会话 id。".into(),
                ],
            );
        }
        "list" => match sess.list_recent_ids(24) {
            Ok(ids) => {
                if ids.is_empty() {
                    push_block(env.model, &["（库中暂无会话）".into()]);
                } else {
                    let mut lines: Vec<String> = vec!["最近会话 id（updated 倒序）：".into()];
                    for id in ids {
                        lines.push(format!("  · {id}"));
                    }
                    push_block(env.model, &lines);
                }
            }
            Err(e) => push_block(env.model, &[format!("列出失败: {e}")]),
        },
        "open" => {
            let target = parts.get(2).copied().unwrap_or("");
            if target.is_empty() {
                push_block(
                    env.model,
                    &["用法: /conv open <id> 或 /conv open last".into()],
                );
                return Ok(true);
            }
            let open_res = if target == "last" {
                let ids = match sess.list_recent_ids(1) {
                    Ok(v) => v,
                    Err(e) => {
                        push_block(env.model, &[format!("列出失败: {e}")]);
                        return Ok(true);
                    }
                };
                let Some(id) = ids.into_iter().next() else {
                    push_block(env.model, &["库中暂无会话。".into()]);
                    return Ok(true);
                };
                sess.switch_conversation(id.as_str(), messages, agent_role_owned)
            } else {
                sess.switch_conversation(target, messages, agent_role_owned)
            };
            match open_res {
                Ok(()) => {
                    push_block(env.model, &[format!("已打开会话 {}", sess.conversation_id)]);
                    {
                        let mut g = env.model.lock().unwrap_or_else(|e| e.into_inner());
                        g.sqlite_conversation_id = Some(sess.conversation_id.clone());
                    }
                    tui_sqlite_slash_refresh_ui(env, messages.as_slice(), agent_role_owned).await;
                }
                Err(e) => push_block(env.model, &[format!("打开失败: {e}")]),
            }
        }
        "new" => {
            let cfg = env.cfg_holder.read().await;
            let rec = Arc::new(ToolOutcomeRecorder::new());
            let bootstrap = workspace_session::repl_bootstrap_messages_fast(
                &cfg,
                agent_role_owned.as_ref().map(|s| s.as_str()),
                &rec,
            );
            drop(cfg);
            let role_snap = agent_role_owned.clone();
            let role_for_save = role_snap.as_deref();
            match sess.start_fresh_conversation(
                bootstrap,
                role_for_save,
                messages,
                agent_role_owned,
            ) {
                Ok(()) => {
                    push_block(env.model, &[format!("新建会话 {}", sess.conversation_id)]);
                    {
                        let mut g = env.model.lock().unwrap_or_else(|e| e.into_inner());
                        g.sqlite_conversation_id = Some(sess.conversation_id.clone());
                    }
                    tui_sqlite_slash_refresh_ui(env, messages.as_slice(), agent_role_owned).await;
                }
                Err(e) => push_block(env.model, &[format!("新建失败: {e}")]),
            }
        }
        _ => {
            push_block(env.model, &[format!("未知子命令 `{sub}`，输入 /conv help")]);
        }
    }

    Ok(true)
}
