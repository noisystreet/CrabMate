//! 共享 `/conv` / `/branch` 斜杠逻辑（REPL / TUI 共用）。

use crate::runtime::cli_sqlite_session::CliSqliteSessionState;
use crate::types::Message;

/// 斜杠处理结果。
pub(crate) enum CliSqliteSlashResult {
    /// 非 `/conv` / `/branch`。
    NotHandled,
    /// 已消费；`lines` 为用户可见输出。
    Handled { lines: Vec<String> },
}

fn no_db_lines() -> Vec<String> {
    vec![
        "未启用 SQLite 会话库。请在配置中设置非空 conversation_store_sqlite_path（与 Web serve 同源）。"
            .into(),
    ]
}

fn help_lines() -> Vec<String> {
    vec![
        "/conv list — 列出最近会话 id".into(),
        "/conv open <id> — 切换会话".into(),
        "/conv open last — 打开最近更新的会话".into(),
        "/conv new — 新建会话（仅 system 引导）".into(),
        "/branch <n> — 截断到用户 ordinal n 之前（Web 同源）".into(),
        "环境变量 CM_CONVERSATION_ID（或 CM_TUI_CONVERSATION_ID / CM_REPL_CONVERSATION_ID）可指定启动会话。"
            .into(),
    ]
}

fn handle_branch_slash(
    parts: &[&str],
    sess: &mut CliSqliteSessionState,
    messages: &mut Vec<Message>,
    agent_role_owned: &mut Option<String>,
) -> CliSqliteSlashResult {
    let ord_s = parts.get(1).copied().unwrap_or("");
    let Ok(ord) = ord_s.parse::<usize>() else {
        return CliSqliteSlashResult::Handled {
            lines: vec![
                "用法: /branch <before_user_ordinal>".into(),
                "ordinal 为 0-based，语义与 Web POST /chat/branch 一致。".into(),
            ],
        };
    };
    match sess.branch_before_user_ordinal(ord, messages, agent_role_owned) {
        Ok(()) => CliSqliteSlashResult::Handled {
            lines: vec![format!(
                "已分支：截断到第 {ord} 条用户消息之前（revision 已递增）。"
            )],
        },
        Err(e) => CliSqliteSlashResult::Handled {
            lines: vec![format!("分支失败: {e}")],
        },
    }
}

fn handle_conv_list(sess: &CliSqliteSessionState) -> CliSqliteSlashResult {
    match sess.list_recent_ids(24) {
        Ok(ids) if ids.is_empty() => CliSqliteSlashResult::Handled {
            lines: vec!["（库中暂无会话）".into()],
        },
        Ok(ids) => {
            let mut lines: Vec<String> = vec!["最近会话 id（updated 倒序）：".into()];
            for id in ids {
                lines.push(format!("  · {id}"));
            }
            CliSqliteSlashResult::Handled { lines }
        }
        Err(e) => CliSqliteSlashResult::Handled {
            lines: vec![format!("列出失败: {e}")],
        },
    }
}

fn handle_conv_open(
    parts: &[&str],
    sess: &mut CliSqliteSessionState,
    messages: &mut Vec<Message>,
    agent_role_owned: &mut Option<String>,
) -> CliSqliteSlashResult {
    let target = parts.get(2).copied().unwrap_or("");
    if target.is_empty() {
        return CliSqliteSlashResult::Handled {
            lines: vec!["用法: /conv open <id> 或 /conv open last".into()],
        };
    }
    let open_res = if target == "last" {
        match sess.list_recent_ids(1) {
            Ok(ids) => {
                let Some(id) = ids.into_iter().next() else {
                    return CliSqliteSlashResult::Handled {
                        lines: vec!["库中暂无会话。".into()],
                    };
                };
                sess.switch_conversation(id.as_str(), messages, agent_role_owned)
            }
            Err(e) => {
                return CliSqliteSlashResult::Handled {
                    lines: vec![format!("列出失败: {e}")],
                };
            }
        }
    } else {
        sess.switch_conversation(target, messages, agent_role_owned)
    };
    match open_res {
        Ok(()) => CliSqliteSlashResult::Handled {
            lines: vec![format!("已打开会话 {}", sess.conversation_id)],
        },
        Err(e) => CliSqliteSlashResult::Handled {
            lines: vec![format!("打开失败: {e}")],
        },
    }
}

fn handle_conv_new(
    bootstrap_for_new: Option<Vec<Message>>,
    sess: &mut CliSqliteSessionState,
    messages: &mut Vec<Message>,
    agent_role_owned: &mut Option<String>,
) -> CliSqliteSlashResult {
    let Some(bootstrap) = bootstrap_for_new else {
        return CliSqliteSlashResult::Handled {
            lines: vec!["内部错误：缺少新建会话引导消息。".into()],
        };
    };
    let role_snap = agent_role_owned.clone();
    match sess.start_fresh_conversation(bootstrap, role_snap.as_deref(), messages, agent_role_owned)
    {
        Ok(()) => CliSqliteSlashResult::Handled {
            lines: vec![format!("新建会话 {}", sess.conversation_id)],
        },
        Err(e) => CliSqliteSlashResult::Handled {
            lines: vec![format!("新建失败: {e}")],
        },
    }
}

fn handle_conv_subcommand(
    sub: &str,
    parts: &[&str],
    sess: &mut CliSqliteSessionState,
    messages: &mut Vec<Message>,
    agent_role_owned: &mut Option<String>,
    bootstrap_for_new: Option<Vec<Message>>,
) -> CliSqliteSlashResult {
    match sub {
        "help" | "?" => CliSqliteSlashResult::Handled {
            lines: help_lines(),
        },
        "list" => handle_conv_list(sess),
        "open" => handle_conv_open(parts, sess, messages, agent_role_owned),
        "new" => handle_conv_new(bootstrap_for_new, sess, messages, agent_role_owned),
        _ => CliSqliteSlashResult::Handled {
            lines: vec![format!("未知子命令 `{sub}`，输入 /conv help")],
        },
    }
}

/// 处理 `/conv` / `/branch`。`bootstrap_for_new` 在 `/conv new` 时提供新会话消息。
pub(crate) fn try_apply_cli_sqlite_slash(
    trimmed: &str,
    sess: Option<&mut CliSqliteSessionState>,
    messages: &mut Vec<Message>,
    agent_role_owned: &mut Option<String>,
    bootstrap_for_new: Option<Vec<Message>>,
) -> CliSqliteSlashResult {
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let cmd = parts.first().copied().unwrap_or("");
    if cmd != "/conv" && cmd != "/branch" {
        return CliSqliteSlashResult::NotHandled;
    }

    let Some(sess) = sess else {
        return CliSqliteSlashResult::Handled {
            lines: no_db_lines(),
        };
    };

    if cmd == "/branch" {
        return handle_branch_slash(&parts, sess, messages, agent_role_owned);
    }

    let sub = parts.get(1).copied().unwrap_or("help");
    handle_conv_subcommand(
        sub,
        &parts,
        sess,
        messages,
        agent_role_owned,
        bootstrap_for_new,
    )
}
