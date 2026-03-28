//! Web `conversation_id` 持久化：可选 **SQLite**（进程重启后续聊）；未配置路径时仍用内存 `HashMap`（`web::app_state`）。

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;

use crate::types::Message;

const TABLE: &str = "crabmate_conversations";

/// Web 会话保存结果（内存与 SQLite 共用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveConversationOutcome {
    Saved,
    Conflict,
}

/// 与内存态 `app_state` 一致：24h TTL、最多条数（仅 SQLite 路径下 `prune` 使用）。
pub const CONVERSATION_STORE_TTL_SECS: u64 = 24 * 3600;
pub const CONVERSATION_STORE_MAX_ENTRIES: usize = 512;

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// 建表（幂等）。
pub fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(&format!(
        r#"
        CREATE TABLE IF NOT EXISTS {TABLE} (
            id TEXT NOT NULL PRIMARY KEY,
            messages_json TEXT NOT NULL,
            revision INTEGER NOT NULL,
            updated_at_unix INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_{TABLE}_updated ON {TABLE}(updated_at_unix);
        "#
    ))?;
    Ok(())
}

/// 打开库文件并迁移（父目录不存在则创建）。
pub fn open_file(path: &Path) -> Result<Connection, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("无法创建会话库目录 {}: {}", parent.display(), e))?;
    }
    let conn = Connection::open(path)
        .map_err(|e| format!("无法打开会话 SQLite {}: {}", path.display(), e))?;
    migrate(&conn).map_err(|e| format!("会话库 schema 初始化失败 {}: {}", path.display(), e))?;
    Ok(conn)
}

fn messages_to_json(messages: &[Message]) -> Result<String, serde_json::Error> {
    serde_json::to_string(&json!({ "version": 1, "messages": messages }))
}

fn messages_from_json(s: &str) -> Result<Vec<Message>, String> {
    let v: serde_json::Value =
        serde_json::from_str(s).map_err(|e| format!("会话 messages_json 解析失败: {e}"))?;
    let arr = v
        .get("messages")
        .and_then(|m| m.as_array())
        .ok_or_else(|| "会话 JSON 缺少 messages 数组".to_string())?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let m: Message = serde_json::from_value(item.clone())
            .map_err(|e| format!("单条消息反序列化失败: {e}"))?;
        out.push(m);
    }
    Ok(out)
}

/// 读取会话；不存在、或超过 TTL 视为无（并删除过期行）。
pub fn load(
    conn: &Connection,
    id: &str,
    ttl_secs: u64,
) -> Result<Option<(Vec<Message>, u64)>, rusqlite::Error> {
    let now = now_unix();
    let row: Option<(String, i64, i64)> = conn
        .query_row(
            &format!("SELECT messages_json, revision, updated_at_unix FROM {TABLE} WHERE id = ?1"),
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((json, revision, updated)) = row else {
        return Ok(None);
    };
    if ttl_secs > 0 && now.saturating_sub(updated) > ttl_secs as i64 {
        conn.execute(&format!("DELETE FROM {TABLE} WHERE id = ?1"), params![id])?;
        return Ok(None);
    }
    let messages = match messages_from_json(&json) {
        Ok(m) => m,
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "会话 {} 消息 JSON 损坏，已删除该行 error={}",
                id,
                e
            );
            conn.execute(&format!("DELETE FROM {TABLE} WHERE id = ?1"), params![id])?;
            return Ok(None);
        }
    };
    let rev = u64::try_from(revision).unwrap_or(0);
    // 刷新访问时间，与内存态「touch」一致
    conn.execute(
        &format!("UPDATE {TABLE} SET updated_at_unix = ?1 WHERE id = ?2"),
        params![now, id],
    )?;
    Ok(Some((messages, rev)))
}

/// 与 `AppState::save_conversation_messages_if_revision` 语义一致。
pub fn save_if_revision(
    conn: &Connection,
    id: &str,
    messages: Vec<Message>,
    expected_revision: Option<u64>,
) -> Result<SaveConversationOutcome, rusqlite::Error> {
    let now = now_unix();
    let json = match messages_to_json(&messages) {
        Ok(j) => j,
        Err(e) => {
            log::error!(
                target: "crabmate",
                "会话 {} 序列化失败（不应发生）: {}",
                id,
                e
            );
            return Ok(SaveConversationOutcome::Conflict);
        }
    };

    if let Some(exp) = expected_revision {
        let n = conn.execute(
            &format!(
                "UPDATE {TABLE} SET messages_json = ?1, revision = revision + 1, updated_at_unix = ?2 WHERE id = ?3 AND revision = ?4"
            ),
            params![json, now, id, exp as i64],
        )?;
        if n == 0 {
            return Ok(SaveConversationOutcome::Conflict);
        }
    } else {
        let exists: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM {TABLE} WHERE id = ?1"),
            params![id],
            |r| r.get(0),
        )?;
        if exists > 0 {
            return Ok(SaveConversationOutcome::Conflict);
        }
        conn.execute(
            &format!(
                "INSERT INTO {TABLE} (id, messages_json, revision, updated_at_unix) VALUES (?1, ?2, 1, ?3)"
            ),
            params![id, json, now],
        )?;
    }
    prune(
        conn,
        CONVERSATION_STORE_TTL_SECS,
        CONVERSATION_STORE_MAX_ENTRIES,
    )?;
    Ok(SaveConversationOutcome::Saved)
}

/// 删除过期行并按条数淘汰最旧记录。
pub fn prune(conn: &Connection, ttl_secs: u64, max_entries: usize) -> Result<(), rusqlite::Error> {
    let now = now_unix();
    if ttl_secs > 0 {
        let cutoff = now - ttl_secs as i64;
        conn.execute(
            &format!("DELETE FROM {TABLE} WHERE updated_at_unix < ?1"),
            params![cutoff],
        )?;
    }
    if max_entries == 0 {
        return Ok(());
    }
    let count: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {TABLE}"), [], |r| r.get(0))?;
    if count <= max_entries as i64 {
        return Ok(());
    }
    let to_drop = count - max_entries as i64;
    conn.execute(
        &format!(
            "DELETE FROM {TABLE} WHERE id IN (SELECT id FROM {TABLE} ORDER BY updated_at_unix ASC LIMIT ?1)"
        ),
        params![to_drop],
    )?;
    Ok(())
}

/// 当前库中的会话条数。
pub fn count(conn: &Connection) -> Result<usize, rusqlite::Error> {
    let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {TABLE}"), [], |r| r.get(0))?;
    Ok(n as usize)
}

fn update_messages_json_if_revision(
    conn: &Connection,
    id: &str,
    expected_revision: u64,
    corrupt_log: &'static str,
    serialize_fail_log: &'static str,
    mut mutate: impl FnMut(&mut Vec<Message>) -> bool,
) -> Result<SaveConversationOutcome, rusqlite::Error> {
    let row: Option<(String, i64)> = conn
        .query_row(
            &format!("SELECT messages_json, revision FROM {TABLE} WHERE id = ?1"),
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((json, revision)) = row else {
        return Ok(SaveConversationOutcome::Conflict);
    };
    let rev = u64::try_from(revision).unwrap_or(0);
    if rev != expected_revision {
        return Ok(SaveConversationOutcome::Conflict);
    }
    let mut messages = match messages_from_json(&json) {
        Ok(m) => m,
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "{} id={} error={}",
                corrupt_log,
                id,
                e
            );
            return Ok(SaveConversationOutcome::Conflict);
        }
    };
    if !mutate(&mut messages) {
        return Ok(SaveConversationOutcome::Saved);
    }
    let now = now_unix();
    let new_json = match messages_to_json(&messages) {
        Ok(j) => j,
        Err(e) => {
            log::error!(
                target: "crabmate",
                "{} id={} error={}",
                serialize_fail_log,
                id,
                e
            );
            return Ok(SaveConversationOutcome::Conflict);
        }
    };
    let n = conn.execute(
        &format!(
            "UPDATE {TABLE} SET messages_json = ?1, revision = revision + 1, updated_at_unix = ?2 WHERE id = ?3 AND revision = ?4"
        ),
        params![new_json, now, id, expected_revision as i64],
    )?;
    if n == 0 {
        return Ok(SaveConversationOutcome::Conflict);
    }
    prune(
        conn,
        CONVERSATION_STORE_TTL_SECS,
        CONVERSATION_STORE_MAX_ENTRIES,
    )?;
    Ok(SaveConversationOutcome::Saved)
}

/// 截断到「第 `ordinal` 条用户消息」之前（`ordinal` 为 0-based：0 表示删掉从首条用户起的尾部，仅保留 system 等）。
/// 用户消息不含长期记忆注入条（`is_long_term_memory_injection`）。
pub fn truncate_before_user_ordinal_if_revision(
    conn: &Connection,
    id: &str,
    user_ordinal: usize,
    expected_revision: u64,
) -> Result<SaveConversationOutcome, rusqlite::Error> {
    update_messages_json_if_revision(
        conn,
        id,
        expected_revision,
        "truncate_before_user 会话 JSON 损坏",
        "truncate_before_user 序列化失败",
        |messages| {
            let mut u = 0usize;
            let mut cut = messages.len();
            for (i, m) in messages.iter().enumerate() {
                if m.role == "user" && !crate::types::is_long_term_memory_injection(m) {
                    if u == user_ordinal {
                        cut = i;
                        break;
                    }
                    u += 1;
                }
            }
            if cut >= messages.len() {
                return false;
            }
            messages.truncate(cut);
            true
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Message;

    #[test]
    fn save_load_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let msgs = vec![
            Message::system_only("s".to_string()),
            Message::user_only("hi".to_string()),
        ];
        assert_eq!(
            save_if_revision(&conn, "c1", msgs.clone(), None).unwrap(),
            SaveConversationOutcome::Saved
        );
        let loaded = load(&conn, "c1", 3600).unwrap().expect("exists");
        assert_eq!(loaded.1, 1);
        assert_eq!(loaded.0.len(), 2);
        assert_eq!(
            save_if_revision(&conn, "c1", msgs.clone(), Some(1)).unwrap(),
            SaveConversationOutcome::Saved
        );
        let loaded2 = load(&conn, "c1", 3600).unwrap().expect("exists");
        assert_eq!(loaded2.1, 2);
    }
}
