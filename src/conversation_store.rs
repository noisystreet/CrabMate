//! Web `conversation_id` 持久化：可选 **SQLite**（进程重启后续聊）；未配置路径时仍用内存 `HashMap`（`web::app_state`）。

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;

use crate::agent::model_context_view::StructuralCommit;
use crate::cm_api_contract::chat::ConversationLayoutMeta;
use crate::types::Message;

mod archive;
#[cfg(test)]
mod tests;

// A3/A4 接线前，lib 构建下暂无调用方；保留原 `conversation_store::…` 路径。
#[allow(unused_imports)]
pub use archive::*;

const TABLE: &str = "crabmate_conversations";

/// 被移出 canonical 的原文区间归档表（见设计文档 §3.8 / §4.1）。
const ARCHIVE_TABLE: &str = "crabmate_conversation_archive";

/// Web 会话保存结果（内存与 SQLite 共用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveConversationOutcome {
    Saved,
    Conflict,
}

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
        CREATE TABLE IF NOT EXISTS {ARCHIVE_TABLE} (
            conversation_id  TEXT    NOT NULL,
            seq              INTEGER NOT NULL,
            reason           TEXT    NOT NULL,
            range_start      INTEGER NOT NULL,
            range_end        INTEGER NOT NULL,
            messages_json    TEXT    NOT NULL,
            codec            TEXT    NOT NULL DEFAULT 'identity',
            created_at_unix  INTEGER NOT NULL,
            PRIMARY KEY (conversation_id, seq)
        );
        CREATE INDEX IF NOT EXISTS idx_{ARCHIVE_TABLE}_conv ON {ARCHIVE_TABLE}(conversation_id);
        "#
    ))?;
    ensure_active_agent_role_column(conn)?;
    ensure_active_session_mode_column(conn)?;
    ensure_layout_meta_json_column(conn)?;
    Ok(())
}

fn ensure_layout_meta_json_column(conn: &Connection) -> Result<(), rusqlite::Error> {
    ensure_text_column(conn, "layout_meta_json")
}

fn ensure_active_agent_role_column(conn: &Connection) -> Result<(), rusqlite::Error> {
    ensure_text_column(conn, "active_agent_role")
}

fn ensure_active_session_mode_column(conn: &Connection) -> Result<(), rusqlite::Error> {
    ensure_text_column(conn, "active_session_mode")
}

fn ensure_text_column(conn: &Connection, column: &str) -> Result<(), rusqlite::Error> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({TABLE})"))?;
    let mut has = false;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            has = true;
            break;
        }
    }
    if !has {
        conn.execute(
            &format!("ALTER TABLE {TABLE} ADD COLUMN {column} TEXT NOT NULL DEFAULT ''"),
            [],
        )?;
    }
    Ok(())
}

fn layout_meta_json_for_messages(conversation_id: &str, messages: &[Message]) -> String {
    match serde_json::to_string(&crate::cm_turn_layout::layout_meta_from_messages(messages)) {
        Ok(s) => s,
        Err(e) => {
            log::error!(
                target: "crabmate",
                "会话 layout 元数据序列化失败 conversation_id={} error={e}",
                conversation_id
            );
            String::new()
        }
    }
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

/// SQLite / 内存会话读出形状。
#[derive(Debug, Clone)]
pub struct LoadedConversation {
    pub messages: Vec<Message>,
    pub revision: u64,
    pub active_agent_role: String,
    pub active_session_mode: String,
    pub layout: Option<ConversationLayoutMeta>,
}

struct ConversationFetchRow {
    json: String,
    revision: i64,
    updated: i64,
    active_role: String,
    active_mode: String,
    layout_meta_json: String,
}

fn layout_meta_from_column(conversation_id: &str, raw: &str) -> Option<ConversationLayoutMeta> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    match serde_json::from_str(t) {
        Ok(meta) => Some(meta),
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "会话 layout_meta_json 解析失败，已忽略 conversation_id={} error={e}",
                conversation_id
            );
            None
        }
    }
}

/// 读取会话；不存在、或超过 TTL 视为无（并删除过期行）。
/// `active_*` 空串视为未设置；`layout` 仅在 `layout_meta_json` 非空且可解析时存在。
pub fn load(
    conn: &Connection,
    id: &str,
    ttl_secs: u64,
) -> Result<Option<LoadedConversation>, rusqlite::Error> {
    let now = now_unix();
    let row: Option<ConversationFetchRow> = conn
        .query_row(
            &format!(
                "SELECT messages_json, revision, updated_at_unix, active_agent_role, active_session_mode, layout_meta_json FROM {TABLE} WHERE id = ?1"
            ),
            params![id],
            |r| {
                Ok(ConversationFetchRow {
                    json: r.get(0)?,
                    revision: r.get(1)?,
                    updated: r.get(2)?,
                    active_role: r.get(3)?,
                    active_mode: r.get(4)?,
                    layout_meta_json: r.get(5)?,
                })
            },
        )
        .optional()?;
    let Some(row) = row else {
        return Ok(None);
    };
    load_row_after_fetch(conn, id, ttl_secs, now, row)
}

fn load_row_after_fetch(
    conn: &Connection,
    id: &str,
    ttl_secs: u64,
    now: i64,
    row: ConversationFetchRow,
) -> Result<Option<LoadedConversation>, rusqlite::Error> {
    if ttl_secs > 0 && now.saturating_sub(row.updated) > ttl_secs as i64 {
        conn.execute(&format!("DELETE FROM {TABLE} WHERE id = ?1"), params![id])?;
        return Ok(None);
    }
    let messages = match messages_from_json(&row.json) {
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
    let rev = u64::try_from(row.revision).unwrap_or(0);
    conn.execute(
        &format!("UPDATE {TABLE} SET updated_at_unix = ?1 WHERE id = ?2"),
        params![now, id],
    )?;
    Ok(Some(LoadedConversation {
        messages,
        revision: rev,
        active_agent_role: row.active_role,
        active_session_mode: row.active_mode,
        layout: layout_meta_from_column(id, &row.layout_meta_json),
    }))
}

/// 与 `AppState::save_conversation_messages_if_revision` 语义一致。
pub fn save_if_revision(
    conn: &Connection,
    id: &str,
    messages: Vec<Message>,
    active_agent_role: Option<&str>,
    active_session_mode: Option<&str>,
    expected_revision: Option<u64>,
) -> Result<SaveConversationOutcome, rusqlite::Error> {
    let now = now_unix();
    let active_col = active_agent_role
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    let mode_col = active_session_mode
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
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
    let layout_json = layout_meta_json_for_messages(id, &messages);

    if let Some(exp) = expected_revision {
        let n = conn.execute(
            &format!(
                "UPDATE {TABLE} SET messages_json = ?1, active_agent_role = ?2, active_session_mode = ?3, layout_meta_json = ?4, revision = revision + 1, updated_at_unix = ?5 WHERE id = ?6 AND revision = ?7"
            ),
            params![json, active_col, mode_col, layout_json, now, id, exp as i64],
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
                "INSERT INTO {TABLE} (id, messages_json, active_agent_role, active_session_mode, layout_meta_json, revision, updated_at_unix) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)"
            ),
            params![id, json, active_col, mode_col, layout_json, now],
        )?;
    }
    Ok(SaveConversationOutcome::Saved)
}

/// A3：canonical 落盘与归档 INSERT **同事务**提交（设计文档 §3.8 / §4.2）。
///
/// 先 `save_if_revision` 落 `Saved`，再于同一事务内追加本次被移出的原文；
/// 任一步失败整体回滚，维持不变式「归档 ∪ canonical 原文 = 完整原始历史」。
#[allow(clippy::too_many_arguments)]
pub fn save_with_archive_if_revision(
    conn: &Connection,
    id: &str,
    messages: Vec<Message>,
    active_agent_role: Option<&str>,
    active_session_mode: Option<&str>,
    expected_revision: Option<u64>,
    archive_rows: &[ArchiveRow],
    codec_min_bytes: usize,
) -> Result<SaveConversationOutcome, rusqlite::Error> {
    let tx = conn.unchecked_transaction()?;
    let outcome = save_if_revision(
        &tx,
        id,
        messages,
        active_agent_role,
        active_session_mode,
        expected_revision,
    )?;
    if outcome == SaveConversationOutcome::Saved && !archive_rows.is_empty() {
        append_archive_rows(&tx, id, archive_rows, codec_min_bytes)?;
    }
    tx.commit()?;
    Ok(outcome)
}

/// 由结构性提交结果构造待归档行（A3，见 §3.8）。
#[must_use]
pub fn archive_rows_from_structural_commit(commit: &StructuralCommit) -> Vec<ArchiveRow> {
    let mut rows = Vec::new();
    if !commit.archived_prefix.is_empty() {
        rows.push(ArchiveRow {
            reason: if commit.prefix_summarized {
                ArchiveReason::Summarized
            } else {
                ArchiveReason::RemovedTurnGroup
            },
            range_start: commit.range_start as i64,
            range_end: commit.range_end as i64,
            messages: commit.archived_prefix.clone(),
        });
    }
    if !commit.archived_artifacts.is_empty() {
        rows.push(ArchiveRow {
            reason: ArchiveReason::Artifact,
            range_start: commit.range_start as i64,
            range_end: commit.range_end as i64,
            messages: commit.archived_artifacts.clone(),
        });
    }
    rows
}

/// 按 id 删除一行（E2E 夹具 `replace` 等；不存在时静默成功）。
pub fn delete_by_id(conn: &Connection, id: &str) -> Result<(), rusqlite::Error> {
    conn.execute(&format!("DELETE FROM {TABLE} WHERE id = ?1"), params![id])?;
    Ok(())
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

/// 全部会话的 `messages_json` 原文（供扫描 `/uploads/` 引用，避免清掉仍在用的附图）。
///
/// A4：同时合并归档表中的被移出原文，确保 canonical 收缩后附图引用不丢失（见 §4.2）。
pub fn list_all_messages_json(conn: &Connection) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!("SELECT messages_json FROM {TABLE}"))?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    match list_all_archive_messages_json(conn) {
        Ok(archived) => out.extend(archived),
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "归档原文读取失败，/uploads 引用扫描仅覆盖 canonical: {e}"
            );
        }
    }
    Ok(out)
}

/// 与 Web `normalize_client_conversation_id` 字符集一致。
pub const CONVERSATION_ID_MAX_LEN: usize = 128;

fn load_messages_at_revision(
    conn: &Connection,
    id: &str,
    expected_revision: u64,
    corrupt_log: &'static str,
) -> Result<Option<Vec<Message>>, rusqlite::Error> {
    let row: Option<(String, i64)> = conn
        .query_row(
            &format!("SELECT messages_json, revision FROM {TABLE} WHERE id = ?1"),
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((json, revision)) = row else {
        return Ok(None);
    };
    let rev = u64::try_from(revision).unwrap_or(0);
    if rev != expected_revision {
        return Ok(None);
    }
    match messages_from_json(&json) {
        Ok(m) => Ok(Some(m)),
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "{} id={} error={}",
                corrupt_log,
                id,
                e
            );
            Ok(None)
        }
    }
}

fn persist_messages_bump_revision(
    conn: &Connection,
    id: &str,
    expected_revision: u64,
    messages: &[Message],
    serialize_fail_log: &'static str,
) -> Result<SaveConversationOutcome, rusqlite::Error> {
    let new_json = match messages_to_json(messages) {
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
    let now = now_unix();
    let layout_json = layout_meta_json_for_messages(id, messages);
    let n = conn.execute(
        &format!(
            "UPDATE {TABLE} SET messages_json = ?1, layout_meta_json = ?2, revision = revision + 1, updated_at_unix = ?3 WHERE id = ?4 AND revision = ?5"
        ),
        params![new_json, layout_json, now, id, expected_revision as i64],
    )?;
    if n == 0 {
        return Ok(SaveConversationOutcome::Conflict);
    }
    Ok(SaveConversationOutcome::Saved)
}

fn update_messages_json_if_revision(
    conn: &Connection,
    id: &str,
    expected_revision: u64,
    corrupt_log: &'static str,
    serialize_fail_log: &'static str,
    mut mutate: impl FnMut(&mut Vec<Message>) -> bool,
) -> Result<SaveConversationOutcome, rusqlite::Error> {
    let Some(mut messages) = load_messages_at_revision(conn, id, expected_revision, corrupt_log)?
    else {
        return Ok(SaveConversationOutcome::Conflict);
    };
    if !mutate(&mut messages) {
        return Ok(SaveConversationOutcome::Saved);
    }
    persist_messages_bump_revision(conn, id, expected_revision, &messages, serialize_fail_log)
}

/// 截断到「第 `ordinal` 条用户消息」之前（`ordinal` 为 0-based：0 表示删掉从首条用户起的尾部，仅保留 system 等）。
/// 计数不含长期记忆 / 变更集 / 首轮工作区画像 / 上下文摘要等注入条（见 [`crate::types::user_message_counts_for_branch_truncation`]）。
///
/// `user_ordinal` 是客户端**会话级**序号（含已归档回合，见 §3.11）。Phase A 不做 re-expand：
/// 先用归档中的真实用户回合数把会话级序号换算为 canonical 内序号；若目标落在归档前缀内
/// （`user_ordinal < archived_users`）则返回 [`SaveConversationOutcome::Conflict`]——
/// 即 §3.8 要求的「归档保留上限被突破时分支接口必须返回显式错误」。
pub fn truncate_before_user_ordinal_if_revision(
    conn: &Connection,
    id: &str,
    user_ordinal: usize,
    expected_revision: u64,
) -> Result<SaveConversationOutcome, rusqlite::Error> {
    let archived_users = archive::archive_real_user_count(conn, id)?;
    let Some(canonical_ordinal) = user_ordinal.checked_sub(archived_users) else {
        log::warn!(
            "分支截断目标序号 {user_ordinal} 落在归档前缀内（已归档真实用户回合 {archived_users} 条）；\
Phase A 暂无 re-expand，返回 Conflict（见设计文档 §3.8/§3.11）"
        );
        return Ok(SaveConversationOutcome::Conflict);
    };
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
                if crate::types::user_message_counts_for_branch_truncation(m) {
                    if u == canonical_ordinal {
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
