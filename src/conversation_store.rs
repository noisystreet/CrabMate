//! Web `conversation_id` 持久化：可选 **SQLite**（进程重启后续聊）；未配置路径时仍用内存 `HashMap`（`web::app_state`）。

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
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

/// 分支截断结果（含「归档前缀不完整」显式错误，见设计文档 §3.11）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchTruncateOutcome {
    Saved,
    Conflict,
    /// 分支点早于归档保留窗口、且归档前缀已被回收，无法安全回放（§3.11）。
    ArchiveIncomplete,
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
    ensure_active_permission_level_column(conn)?;
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

/// 会话级权限等级**意图**（设计 §16.1 的「持久化」段）。
///
/// 空串 = 该会话从未指定：读回时必须回落配置默认，**不得**当成 `approved` 落死——
/// 否则运维把 `permission_level_max` 调低后，旧会话会按旧值续跑，降级失效。
fn ensure_active_permission_level_column(conn: &Connection) -> Result<(), rusqlite::Error> {
    ensure_text_column(conn, "active_permission_level")
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
    /// 会话级权限等级意图；空串 = 未指定（调用方须回落配置默认）。
    pub active_permission_level: String,
    pub layout: Option<ConversationLayoutMeta>,
}

struct ConversationFetchRow {
    json: String,
    revision: i64,
    updated: i64,
    active_role: String,
    active_mode: String,
    active_permission_level: String,
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
                "SELECT messages_json, revision, updated_at_unix, active_agent_role, active_session_mode, active_permission_level, layout_meta_json FROM {TABLE} WHERE id = ?1"
            ),
            params![id],
            |r| {
                Ok(ConversationFetchRow {
                    json: r.get(0)?,
                    revision: r.get(1)?,
                    updated: r.get(2)?,
                    active_role: r.get(3)?,
                    active_mode: r.get(4)?,
                    active_permission_level: r.get(5)?,
                    layout_meta_json: r.get(6)?,
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
        active_permission_level: row.active_permission_level,
        layout: layout_meta_from_column(id, &row.layout_meta_json),
    }))
}

/// `active_*` 列的写入意图：`None` 或空白 = **保持原值**（该列不进 `UPDATE` 的 SET 子句）。
///
/// 早期实现把 `None` 落成空串，于是「回合结束的整行覆盖」会拿回合开始时读到的旧快照，
/// 把期间 `POST /conversation/state` 写入的新值改回去——用户切了档、跑完一轮又被复位。
#[must_use]
pub fn active_state_column(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|s| !s.is_empty())
}

/// 把三列 `active_*` 意图收进 `SET` 子句：只加真正要写的列，其余列保持原值。
fn push_active_assigns(
    assigns: &mut Vec<&'static str>,
    values: &mut Vec<Value>,
    fields: [(&'static str, Option<&str>); 3],
) {
    for (assign, raw) in fields {
        if let Some(v) = active_state_column(raw) {
            assigns.push(assign);
            values.push(Value::Text(v.to_string()));
        }
    }
}

/// 与 `AppState::save_conversation_messages_if_revision` 语义一致。
///
/// `active_*` 三参传 `None` = 保持该列原值（见 [`active_state_column`]），
/// 回合结束的落盘因此不会覆盖期间 `POST /conversation/state` 写入的新值。
pub fn save_if_revision(
    conn: &Connection,
    id: &str,
    messages: Vec<Message>,
    active_agent_role: Option<&str>,
    active_session_mode: Option<&str>,
    active_permission_level: Option<&str>,
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
    let layout_json = layout_meta_json_for_messages(id, &messages);
    let active_fields = [
        ("active_agent_role = ?", active_agent_role),
        ("active_session_mode = ?", active_session_mode),
        ("active_permission_level = ?", active_permission_level),
    ];

    if let Some(exp) = expected_revision {
        let mut assigns: Vec<&'static str> = vec!["messages_json = ?", "layout_meta_json = ?"];
        let mut values: Vec<Value> = vec![Value::Text(json), Value::Text(layout_json)];
        push_active_assigns(&mut assigns, &mut values, active_fields);
        assigns.push("revision = revision + 1");
        assigns.push("updated_at_unix = ?");
        values.push(Value::Integer(now));
        values.push(Value::Text(id.to_string()));
        values.push(Value::Integer(exp as i64));
        let sql = format!(
            "UPDATE {TABLE} SET {} WHERE id = ? AND revision = ?",
            assigns.join(", ")
        );
        let n = conn.execute(&sql, rusqlite::params_from_iter(values))?;
        if n == 0 {
            return Ok(SaveConversationOutcome::Conflict);
        }
        return Ok(SaveConversationOutcome::Saved);
    }

    let exists: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM {TABLE} WHERE id = ?1"),
        params![id],
        |r| r.get(0),
    )?;
    if exists > 0 {
        return Ok(SaveConversationOutcome::Conflict);
    }
    // 新行三列是 `NOT NULL DEFAULT ''`，未申请时写空串，等价于「该会话从未指定」。
    conn.execute(
        &format!(
            "INSERT INTO {TABLE} (id, messages_json, active_agent_role, active_session_mode, active_permission_level, layout_meta_json, revision, updated_at_unix) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)"
        ),
        params![
            id,
            json,
            active_state_column(active_agent_role).unwrap_or(""),
            active_state_column(active_session_mode).unwrap_or(""),
            active_state_column(active_permission_level).unwrap_or(""),
            layout_json,
            now
        ],
    )?;
    Ok(SaveConversationOutcome::Saved)
}

/// 只改会话级 `active_*` 意图列：**不**碰 `messages_json`，**不** bump `revision`、**不**动 `updated_at_unix`。
///
/// 与 [`save_if_revision`]（整行覆盖 + CAS）分工：那条写消息，这条给 `POST /conversation/state` 用。
/// 故意不 bump `revision`——进行中的回合结束时拿「回合开始读到的 revision」做 CAS，
/// 这里一 bump 就会把那轮落盘判成 `Conflict`：用户只是切了个开关，整轮对话却不落盘。
///
/// 返回 `false` 表示该会话不存在（调用方据此回 404）。
pub fn set_active_state(
    conn: &Connection,
    id: &str,
    active_agent_role: Option<&str>,
    active_session_mode: Option<&str>,
    active_permission_level: Option<&str>,
) -> Result<bool, rusqlite::Error> {
    let mut assigns: Vec<&'static str> = Vec::new();
    let mut values: Vec<Value> = Vec::new();
    push_active_assigns(
        &mut assigns,
        &mut values,
        [
            ("active_agent_role = ?", active_agent_role),
            ("active_session_mode = ?", active_session_mode),
            ("active_permission_level = ?", active_permission_level),
        ],
    );
    if assigns.is_empty() {
        // 三列都没申请：只回答「会话在不在」，避免长出空的 `SET` 子句。
        let exists: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM {TABLE} WHERE id = ?1"),
            params![id],
            |r| r.get(0),
        )?;
        return Ok(exists > 0);
    }
    values.push(Value::Text(id.to_string()));
    let sql = format!("UPDATE {TABLE} SET {} WHERE id = ?", assigns.join(", "));
    let n = conn.execute(&sql, rusqlite::params_from_iter(values))?;
    Ok(n > 0)
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
    active_permission_level: Option<&str>,
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
        active_permission_level,
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

/// 删除过期行并按条数淘汰最旧记录；**同事务**级联回收归档行（设计文档 §4.2 / §5.3.2 C4）。
///
/// - 会话被删（TTL / 超 `max_entries`）时，其归档行一并删除；
/// - `archive_max_rows_per_conversation` 控制单会话归档行数上限（`0` 不限），
///   与 TTL 维度一同交给 [`archive::prune_archive`] 处理。
pub fn prune(
    conn: &Connection,
    ttl_secs: u64,
    max_entries: usize,
    archive_max_rows_per_conversation: usize,
) -> Result<(), rusqlite::Error> {
    let tx = conn.unchecked_transaction()?;
    if ttl_secs > 0 {
        prune_expired_by_ttl(&tx, now_unix() - ttl_secs as i64)?;
    }
    if max_entries > 0 {
        prune_overflow_entries(&tx, max_entries)?;
    }
    archive::prune_archive(&tx, ttl_secs, archive_max_rows_per_conversation)?;
    tx.commit()?;
    Ok(())
}

/// TTL 维度：先删归档（会话表未变故子查询稳定），再删会话，保证不遗留孤儿归档行。
fn prune_expired_by_ttl(tx: &Connection, cutoff: i64) -> Result<(), rusqlite::Error> {
    tx.execute(
        &format!(
            "DELETE FROM {ARCHIVE_TABLE} WHERE conversation_id IN (SELECT id FROM {TABLE} WHERE updated_at_unix < ?1)"
        ),
        params![cutoff],
    )?;
    tx.execute(
        &format!("DELETE FROM {TABLE} WHERE updated_at_unix < ?1"),
        params![cutoff],
    )?;
    Ok(())
}

/// 条数维度：超出 `max_entries` 时按 `updated_at_unix` 升序淘汰最旧会话（连同其归档行）。
fn prune_overflow_entries(tx: &Connection, max_entries: usize) -> Result<(), rusqlite::Error> {
    let count: i64 = tx.query_row(&format!("SELECT COUNT(*) FROM {TABLE}"), [], |r| r.get(0))?;
    if count <= max_entries as i64 {
        return Ok(());
    }
    let to_drop = count - max_entries as i64;
    // 同上：先删归档，再删会话。
    tx.execute(
        &format!(
            "DELETE FROM {ARCHIVE_TABLE} WHERE conversation_id IN (SELECT id FROM {TABLE} ORDER BY updated_at_unix ASC LIMIT ?1)"
        ),
        params![to_drop],
    )?;
    tx.execute(
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

/// 返回第 `ordinal` 条**真实用户**消息在 `messages` 中的下标（0-based）。
/// 不足 `ordinal + 1` 条真实用户时返回 `messages.len()`（表示无需截断）。
pub(crate) fn branch_cut_index(messages: &[Message], ordinal: usize) -> usize {
    let mut u = 0usize;
    for (i, m) in messages.iter().enumerate() {
        if crate::types::user_message_counts_for_branch_truncation(m) {
            if u == ordinal {
                return i;
            }
            u += 1;
        }
    }
    messages.len()
}

/// 把归档原文重放回 canonical，得**完整原文历史**（re-expand，见 §3.11）：
/// - 先剔除 canonical 中的基线摘要注入条（re-expand 后不再保留基线摘要）；
/// - 再把归档原文拼在「首个真实用户之前」（无真实用户时退化为「首个非 `system` 条之前」，
///   再退化为末尾），使归档前缀回到 canonical 早期真实用户之前。
fn reexpand_canonical(canonical: &[Message], archived_original: Vec<Message>) -> Vec<Message> {
    let mut stripped: Vec<Message> = canonical
        .iter()
        .filter(|m| !crate::types::is_context_summary_injection(m))
        .cloned()
        .collect();
    let insert_at = stripped
        .iter()
        .position(crate::types::user_message_counts_for_branch_truncation)
        .or_else(|| stripped.iter().position(|m| m.role != "system"))
        .unwrap_or(stripped.len());
    let mut out = Vec::with_capacity(stripped.len() + archived_original.len());
    out.extend(stripped.drain(..insert_at));
    out.extend(archived_original);
    out.extend(stripped);
    out
}

/// 截断到「第 `user_ordinal` 条用户消息」之前（`ordinal` 为 0-based：0 表示删掉从首条用户起的尾部，仅保留 system 等）。
/// 计数不含长期记忆 / 变更集 / 首轮工作区画像 / 上下文摘要等注入条（见 [`crate::types::user_message_counts_for_branch_truncation`]）。
///
/// `user_ordinal` 是客户端**会话级**序号（含已归档回合，见 §3.11）：
/// - 分支点落在 canonical 保留窗口内（`user_ordinal >= 已归档真实用户数`）时，换算为 canonical 内序号直接截断；
/// - 分支点落在归档前缀内时做 **re-expand**（把归档原文重放回 canonical 得完整原文历史后按会话级序号截断），
///   并在同一事务内清除该会话归档（已被重放消费／属于被丢弃的未来）；
/// - 若归档前缀已被回收（`MIN(seq) > 0`），归档不再是完整原始前缀：会话级序号换算不可信、re-expand 亦不可能，
///   故**两个分支**都应返回 [`BranchTruncateOutcome::ArchiveIncomplete`]（显式错误，绝不静默截错，见 §3.8/§5.3.6）。
pub fn truncate_before_user_ordinal_if_revision(
    conn: &Connection,
    id: &str,
    user_ordinal: usize,
    expected_revision: u64,
) -> Result<BranchTruncateOutcome, rusqlite::Error> {
    let tx = conn.unchecked_transaction()?;
    let Some(canonical) = load_messages_at_revision(
        &tx,
        id,
        expected_revision,
        "truncate_before_user 会话 JSON 损坏",
    )?
    else {
        return Ok(BranchTruncateOutcome::Conflict);
    };
    if archive::archive_prefix_incomplete(&tx, id)? {
        log::warn!(
            "分支截断目标序号 {user_ordinal} 对应会话的归档前缀已被回收（MIN(seq) > 0），\
归档不再是完整原始前缀、序号换算不可信；返回 ArchiveIncomplete（见设计文档 §3.11）"
        );
        return Ok(BranchTruncateOutcome::ArchiveIncomplete);
    }
    let archived_users = archive::archive_real_user_count(&tx, id)?;
    if user_ordinal >= archived_users {
        // 分支点落在 canonical 保留窗口内：无需 re-expand，归档不动。
        truncate_within_canonical(
            tx,
            id,
            canonical,
            user_ordinal - archived_users,
            expected_revision,
        )
    } else {
        truncate_and_clear_archive(tx, id, &canonical, user_ordinal, expected_revision)
    }
}

/// 落盘并 bump revision；返回是否成功（`false` 为并发冲突，事务由调用方回滚）。
fn persist_truncated_or_conflict(
    tx: &Connection,
    id: &str,
    expected_revision: u64,
    messages: &[Message],
) -> Result<bool, rusqlite::Error> {
    Ok(persist_messages_bump_revision(
        tx,
        id,
        expected_revision,
        messages,
        "truncate_before_user 序列化失败",
    )? == SaveConversationOutcome::Saved)
}

/// 分支点落在 canonical 保留窗口内：按 canonical 内序号直接截断并落盘（归档不动）。
fn truncate_within_canonical(
    tx: Transaction<'_>,
    id: &str,
    canonical: Vec<Message>,
    canonical_ordinal: usize,
    expected_revision: u64,
) -> Result<BranchTruncateOutcome, rusqlite::Error> {
    let cut = branch_cut_index(&canonical, canonical_ordinal);
    if cut >= canonical.len() {
        return Ok(BranchTruncateOutcome::Saved);
    }
    let mut truncated = canonical;
    truncated.truncate(cut);
    if !persist_truncated_or_conflict(&tx, id, expected_revision, &truncated)? {
        return Ok(BranchTruncateOutcome::Conflict);
    }
    tx.commit()?;
    Ok(BranchTruncateOutcome::Saved)
}

/// 分支点落在归档前缀内：re-expand 后按会话级序号截断，并在同一事务内清除该会话归档
/// （已被重放消费／属于被丢弃的未来）。**调用方已确认归档为完整原始前缀**（见 `archive_prefix_incomplete`）。
fn truncate_and_clear_archive(
    tx: Transaction<'_>,
    id: &str,
    canonical: &[Message],
    user_ordinal: usize,
    expected_revision: u64,
) -> Result<BranchTruncateOutcome, rusqlite::Error> {
    let archived_original = archive::load_archive_original_messages(&tx, id)?;
    let mut full = reexpand_canonical(canonical, archived_original);
    full.truncate(branch_cut_index(&full, user_ordinal));
    if !persist_truncated_or_conflict(&tx, id, expected_revision, &full)? {
        return Ok(BranchTruncateOutcome::Conflict);
    }
    archive::delete_archive_for_conversation(&tx, id)?;
    tx.commit()?;
    Ok(BranchTruncateOutcome::Saved)
}
