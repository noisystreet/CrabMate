//! CLI/TUI 共用的会话 SQLite：与 Web **`conversation_store_sqlite_path`** 同源。

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::conversation_store::{
    self, CONVERSATION_STORE_TTL_SECS, SaveConversationOutcome, validate_conversation_id_chars,
};
use crate::memory::long_term_memory::LongTermMemoryRuntime;
use crate::types::Message;

fn new_conversation_id() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let pid = std::process::id();
    format!("cli-{ms}-{pid}")
}

fn open_db(
    path: &Path,
) -> Result<Arc<Mutex<Connection>>, Box<dyn std::error::Error + Send + Sync>> {
    let conn = conversation_store::open_file(path)?;
    LongTermMemoryRuntime::migrate_on_connection(&conn)?;
    Ok(Arc::new(Mutex::new(conn)))
}

fn resolve_bootstrap_conversation_id(
    preferred_id: Option<&str>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match preferred_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => {
            validate_conversation_id_chars(id)?;
            Ok(id.to_string())
        }
        None => Ok(new_conversation_id()),
    }
}

type BootstrapLoadedSession = (Vec<Message>, u64, String);

fn bootstrap_try_load_existing(
    conn: &Arc<Mutex<Connection>>,
    conversation_id: &str,
) -> Result<Option<BootstrapLoadedSession>, Box<dyn std::error::Error + Send + Sync>> {
    let g = conn.lock().map_err(|e| format!("会话库锁: {e}"))?;
    Ok(
        conversation_store::load(&g, conversation_id, CONVERSATION_STORE_TTL_SECS)?
            .map(|(msgs, rev, role, _mode)| (msgs, rev, role)),
    )
}

fn bootstrap_insert_and_reload(
    conn: &Arc<Mutex<Connection>>,
    conversation_id: &str,
    bootstrap_messages: Vec<Message>,
    active: Option<&str>,
) -> Result<BootstrapLoadedSession, Box<dyn std::error::Error + Send + Sync>> {
    let g = conn.lock().map_err(|e| format!("会话库锁: {e}"))?;
    let outcome = conversation_store::save_if_revision(
        &g,
        conversation_id,
        bootstrap_messages,
        active,
        None,
        None,
    )?;
    drop(g);
    if outcome != SaveConversationOutcome::Saved {
        return Err("新建会话写入 SQLite 失败（冲突）".into());
    }

    let g = conn.lock().map_err(|e| format!("会话库锁: {e}"))?;
    let loaded = conversation_store::load(&g, conversation_id, CONVERSATION_STORE_TTL_SECS)?
        .ok_or_else(|| "新建会话读回失败".to_string())?;
    drop(g);
    Ok((loaded.0, loaded.1, loaded.2))
}

/// 终端侧（REPL / TUI）SQLite 会话状态。
pub(crate) struct CliSqliteSessionState {
    conn: Arc<Mutex<Connection>>,
    pub(crate) conversation_id: String,
    revision: Option<u64>,
    pub(crate) persisted_role: String,
}

impl CliSqliteSessionState {
    /// 打开库并绑定 `preferred_id`；若无此行则以 `bootstrap_messages` 插入新会话。
    pub(crate) fn bootstrap(
        db_path: &Path,
        preferred_id: Option<&str>,
        bootstrap_messages: Vec<Message>,
        agent_role_label: Option<&str>,
    ) -> Result<(Self, Vec<Message>), Box<dyn std::error::Error + Send + Sync>> {
        let conn = open_db(db_path)?;
        let conversation_id = resolve_bootstrap_conversation_id(preferred_id)?;
        let active = agent_role_label.map(str::trim).filter(|s| !s.is_empty());

        if let Some((msgs, rev, role)) =
            bootstrap_try_load_existing(&conn, conversation_id.as_str())?
        {
            return Ok((
                Self {
                    conn,
                    conversation_id,
                    revision: Some(rev),
                    persisted_role: role,
                },
                msgs,
            ));
        }

        let (msgs, rev, role) = bootstrap_insert_and_reload(
            &conn,
            conversation_id.as_str(),
            bootstrap_messages,
            active,
        )?;

        Ok((
            Self {
                conn,
                conversation_id,
                revision: Some(rev),
                persisted_role: role,
            },
            msgs,
        ))
    }

    pub(crate) fn persist_round(
        &mut self,
        messages: &[Message],
        agent_role_label: Option<&str>,
    ) -> Result<(), String> {
        let active = agent_role_label.map(str::trim).filter(|s| !s.is_empty());
        let exp = self
            .revision
            .ok_or_else(|| "会话 revision 未知（不应发生）；请重新打开会话".to_string())?;
        let g = self.conn.lock().map_err(|e| format!("会话库锁: {e}"))?;
        let outcome = conversation_store::save_if_revision(
            &g,
            self.conversation_id.as_str(),
            messages.to_vec(),
            active,
            None,
            Some(exp),
        )
        .map_err(|e| e.to_string())?;
        drop(g);
        match outcome {
            SaveConversationOutcome::Saved => {
                let g = self.conn.lock().map_err(|e| format!("会话库锁: {e}"))?;
                let loaded = conversation_store::load(
                    &g,
                    self.conversation_id.as_str(),
                    CONVERSATION_STORE_TTL_SECS,
                )
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "保存后读回失败".to_string())?;
                drop(g);
                self.revision = Some(loaded.1);
                self.persisted_role = loaded.2;
                Ok(())
            }
            SaveConversationOutcome::Conflict => {
                Err("会话 revision 冲突（其它进程已更新）；请 /conv open <id> 重新加载".to_string())
            }
        }
    }

    pub(crate) fn branch_before_user_ordinal(
        &mut self,
        before_user_ordinal: usize,
        messages_out: &mut Vec<Message>,
        agent_role_owned: &mut Option<String>,
    ) -> Result<(), String> {
        let exp = self
            .revision
            .ok_or_else(|| "缺少 revision，无法分支".to_string())?;
        let g = self.conn.lock().map_err(|e| format!("会话库锁: {e}"))?;
        let outcome = conversation_store::truncate_before_user_ordinal_if_revision(
            &g,
            self.conversation_id.as_str(),
            before_user_ordinal,
            exp,
        )
        .map_err(|e| e.to_string())?;
        drop(g);
        if outcome != SaveConversationOutcome::Saved {
            return Err("分支失败：revision 不匹配或 ordinal 超出范围".to_string());
        }
        self.reload_messages(messages_out, agent_role_owned)
    }

    pub(crate) fn reload_messages(
        &mut self,
        messages_out: &mut Vec<Message>,
        agent_role_owned: &mut Option<String>,
    ) -> Result<(), String> {
        let g = self.conn.lock().map_err(|e| format!("会话库锁: {e}"))?;
        let loaded = conversation_store::load(
            &g,
            self.conversation_id.as_str(),
            CONVERSATION_STORE_TTL_SECS,
        )
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "会话不存在或已过期".to_string())?;
        drop(g);
        *messages_out = loaded.0;
        self.revision = Some(loaded.1);
        self.persisted_role = loaded.2.clone();
        if self.persisted_role.trim().is_empty() {
            *agent_role_owned = None;
        } else {
            *agent_role_owned = Some(self.persisted_role.clone());
        }
        Ok(())
    }

    pub(crate) fn switch_conversation(
        &mut self,
        id: &str,
        messages_out: &mut Vec<Message>,
        agent_role_owned: &mut Option<String>,
    ) -> Result<(), String> {
        validate_conversation_id_chars(id)?;
        self.conversation_id = id.trim().to_string();
        self.reload_messages(messages_out, agent_role_owned)
    }

    pub(crate) fn start_fresh_conversation(
        &mut self,
        bootstrap: Vec<Message>,
        agent_role_label: Option<&str>,
        messages_out: &mut Vec<Message>,
        agent_role_owned: &mut Option<String>,
    ) -> Result<(), String> {
        let id = new_conversation_id();
        let active = agent_role_label.map(str::trim).filter(|s| !s.is_empty());
        let g = self.conn.lock().map_err(|e| format!("会话库锁: {e}"))?;
        let outcome =
            conversation_store::save_if_revision(&g, id.as_str(), bootstrap, active, None, None)
                .map_err(|e| e.to_string())?;
        drop(g);
        if outcome != SaveConversationOutcome::Saved {
            return Err("新建会话写入失败".to_string());
        }
        self.conversation_id = id;
        let g = self.conn.lock().map_err(|e| format!("会话库锁: {e}"))?;
        let loaded = conversation_store::load(
            &g,
            self.conversation_id.as_str(),
            CONVERSATION_STORE_TTL_SECS,
        )
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "读回新会话失败".to_string())?;
        drop(g);
        self.revision = Some(loaded.1);
        self.persisted_role = loaded.2.clone();
        *messages_out = loaded.0;
        if self.persisted_role.trim().is_empty() {
            *agent_role_owned = None;
        } else {
            *agent_role_owned = Some(self.persisted_role.clone());
        }
        Ok(())
    }

    pub(crate) fn list_recent_ids(&self, limit: usize) -> Result<Vec<String>, String> {
        let g = self.conn.lock().map_err(|e| format!("会话库锁: {e}"))?;
        conversation_store::list_conversation_ids_recent_first(&g, limit).map_err(|e| e.to_string())
    }

    /// 最近会话：id + Tauri 同源展示标题。
    pub(crate) fn list_recent_entries(
        &self,
        limit: usize,
    ) -> Result<Vec<crate::conversation_store::ConversationListEntry>, String> {
        let g = self.conn.lock().map_err(|e| format!("会话库锁: {e}"))?;
        conversation_store::list_conversations_recent_first(&g, limit).map_err(|e| e.to_string())
    }
}

fn preferred_conversation_id_from_env() -> Option<String> {
    for key in [
        "CM_CONVERSATION_ID",
        "CM_TUI_CONVERSATION_ID",
        "CM_REPL_CONVERSATION_ID",
    ] {
        if let Ok(v) = std::env::var(key)
            && !v.trim().is_empty()
        {
            return Some(v);
        }
    }
    None
}

/// 若配置了会话 SQLite 路径则 bootstrap；否则原样返回。
pub(crate) async fn maybe_bootstrap_cli_sqlite(
    cfg_holder: &crate::config::SharedAgentConfig,
    messages: Vec<Message>,
    agent_role_owned: Option<String>,
) -> Result<(Option<CliSqliteSessionState>, Vec<Message>, Option<String>), std::io::Error> {
    let conv_path = cfg_holder
        .read()
        .await
        .conversation_persistence
        .conversation_store_sqlite_path
        .clone();
    if conv_path.trim().is_empty() {
        return Ok((None, messages, agent_role_owned));
    }
    let p = std::path::Path::new(conv_path.trim());
    let preferred = preferred_conversation_id_from_env();
    let (st, msgs) = CliSqliteSessionState::bootstrap(
        p,
        preferred.as_deref(),
        messages,
        agent_role_owned.as_deref(),
    )
    .map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("会话 SQLite 初始化失败: {e}"),
        )
    })?;
    let role_out = if st.persisted_role.trim().is_empty() {
        None
    } else {
        Some(st.persisted_role.clone())
    };
    Ok((Some(st), msgs, role_out))
}
