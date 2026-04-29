//! 长期记忆：SQLite 表（可与会话库同文件或独立文件）。
//!
//! 列含可选 TTL（`expires_at_unix`）、标签 JSON、来源（`auto` / `explicit`）；过期行在读取与写入前清理。

use std::path::Path;

use rusqlite::{Connection, params};

const TABLE: &str = "crabmate_long_term_memory";

/// 一行记忆（检索结果用；部分字段供未来扩展/调试保留）。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MemoryRow {
    pub id: i64,
    pub chunk_text: String,
    pub source_role: String,
    pub created_at_unix: i64,
    pub expires_at_unix: Option<i64>,
    pub tags_json: String,
    pub embedding: Option<Vec<u8>>,
}

fn ensure_column(conn: &Connection, col: &str, ddl: &str) -> Result<(), rusqlite::Error> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({TABLE})"))?;
    let mut has = false;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == col {
            has = true;
            break;
        }
    }
    if !has {
        conn.execute(ddl, [])?;
    }
    Ok(())
}

/// 建表（幂等）；与会话库共用连接时应在 `conversation_store::migrate` 之后调用。
pub fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(&format!(
        r#"
        CREATE TABLE IF NOT EXISTS {TABLE} (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scope_id TEXT NOT NULL,
            chunk_text TEXT NOT NULL,
            source_role TEXT NOT NULL,
            created_at_unix INTEGER NOT NULL,
            embedding BLOB
        );
        CREATE INDEX IF NOT EXISTS idx_{TABLE}_scope_created ON {TABLE}(scope_id, created_at_unix DESC);
        "#
    ))?;
    ensure_column(
        conn,
        "expires_at_unix",
        &format!("ALTER TABLE {TABLE} ADD COLUMN expires_at_unix INTEGER"),
    )?;
    ensure_column(
        conn,
        "tags_json",
        &format!("ALTER TABLE {TABLE} ADD COLUMN tags_json TEXT NOT NULL DEFAULT '[]'"),
    )?;
    ensure_column(
        conn,
        "source_kind",
        &format!("ALTER TABLE {TABLE} ADD COLUMN source_kind TEXT NOT NULL DEFAULT 'auto'"),
    )?;
    Ok(())
}

pub fn open_file(path: &Path) -> Result<Connection, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("无法创建长期记忆库目录 {}: {}", parent.display(), e))?;
    }
    let conn = Connection::open(path)
        .map_err(|e| format!("无法打开长期记忆 SQLite {}: {}", path.display(), e))?;
    migrate(&conn).map_err(|e| format!("长期记忆 schema 初始化失败 {}: {}", path.display(), e))?;
    Ok(conn)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// 删除某作用域内已过期行。
pub fn delete_expired_for_scope(
    conn: &Connection,
    scope_id: &str,
    now_unix: i64,
) -> Result<usize, rusqlite::Error> {
    let n = conn.execute(
        &format!(
            "DELETE FROM {TABLE} WHERE scope_id = ?1 AND expires_at_unix IS NOT NULL AND expires_at_unix <= ?2"
        ),
        params![scope_id, now_unix],
    )?;
    Ok(n)
}

/// 列顺序与 [`MemoryRow`] 一致；过滤已过期行。
pub fn list_for_scope(
    conn: &Connection,
    scope_id: &str,
    limit: usize,
) -> Result<Vec<MemoryRow>, rusqlite::Error> {
    let now = now_unix();
    let _ = delete_expired_for_scope(conn, scope_id, now)?;
    let lim = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT id, chunk_text, source_role, created_at_unix, expires_at_unix, tags_json, embedding FROM {TABLE} \
         WHERE scope_id = ?1 AND (expires_at_unix IS NULL OR expires_at_unix > ?3) \
         ORDER BY created_at_unix DESC LIMIT ?2"
    ))?;
    let rows = stmt.query_map(params![scope_id, lim, now], |r| {
        let emb: Option<Vec<u8>> = r.get(6)?;
        Ok(MemoryRow {
            id: r.get(0)?,
            chunk_text: r.get(1)?,
            source_role: r.get(2)?,
            created_at_unix: r.get(3)?,
            expires_at_unix: r.get(4)?,
            tags_json: r
                .get::<_, Option<String>>(5)?
                .unwrap_or_else(|| "[]".to_string()),
            embedding: emb,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 自动索引写入（回合结束）；`source_kind` 为 `auto`。`expires_at_unix` 为 `None` 表示不过期。
pub fn insert_chunk(
    conn: &Connection,
    scope_id: &str,
    chunk_text: &str,
    source_role: &str,
    expires_at_unix: Option<i64>,
    embedding: Option<&[u8]>,
) -> Result<(), rusqlite::Error> {
    let now = now_unix();
    let _ = delete_expired_for_scope(conn, scope_id, now)?;
    conn.execute(
        &format!(
            "INSERT INTO {TABLE} (scope_id, chunk_text, source_role, created_at_unix, expires_at_unix, tags_json, source_kind, embedding) \
             VALUES (?1, ?2, ?3, ?4, ?5, '[]', 'auto', ?6)"
        ),
        params![
            scope_id,
            chunk_text,
            source_role,
            now,
            expires_at_unix,
            embedding
        ],
    )?;
    Ok(())
}

/// 显式记忆（工具 `long_term_remember`）；`tags_json` 为 JSON 数组字符串。
pub fn insert_explicit_chunk(
    conn: &Connection,
    scope_id: &str,
    chunk_text: &str,
    tags_json: &str,
    expires_at_unix: Option<i64>,
    embedding: Option<&[u8]>,
) -> Result<i64, rusqlite::Error> {
    let now = now_unix();
    let _ = delete_expired_for_scope(conn, scope_id, now)?;
    conn.execute(
        &format!(
            "INSERT INTO {TABLE} (scope_id, chunk_text, source_role, created_at_unix, expires_at_unix, tags_json, source_kind, embedding) \
             VALUES (?1, ?2, 'explicit', ?3, ?4, ?5, 'explicit', ?6)"
        ),
        params![scope_id, chunk_text, now, expires_at_unix, tags_json, embedding],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn delete_oldest_beyond(
    conn: &Connection,
    scope_id: &str,
    keep: usize,
) -> Result<(), rusqlite::Error> {
    let now = now_unix();
    let _ = delete_expired_for_scope(conn, scope_id, now)?;
    let keep = i64::try_from(keep).unwrap_or(i64::MAX);
    conn.execute(
        &format!(
            "DELETE FROM {TABLE} WHERE scope_id = ?1 AND id NOT IN (
                SELECT id FROM {TABLE} WHERE scope_id = ?1 \
                AND (expires_at_unix IS NULL OR expires_at_unix > ?3) \
                ORDER BY created_at_unix DESC LIMIT ?2
            )"
        ),
        params![scope_id, keep, now],
    )?;
    Ok(())
}

/// 若存在相同 `chunk_text` 的**未过期**行则跳过插入（简单去重）。
pub fn has_duplicate_text(
    conn: &Connection,
    scope_id: &str,
    chunk_text: &str,
) -> Result<bool, rusqlite::Error> {
    let now = now_unix();
    let n: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM {TABLE} WHERE scope_id = ?1 AND chunk_text = ?2 \
             AND (expires_at_unix IS NULL OR expires_at_unix > ?3)"
        ),
        params![scope_id, chunk_text, now],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// 按主键删除（`long_term_forget`）；仅匹配 `scope_id`。
pub fn delete_by_id_for_scope(
    conn: &Connection,
    scope_id: &str,
    id: i64,
) -> Result<usize, rusqlite::Error> {
    let n = conn.execute(
        &format!("DELETE FROM {TABLE} WHERE scope_id = ?1 AND id = ?2"),
        params![scope_id, id],
    )?;
    Ok(n)
}

/// 删除正文完全匹配的行（可选仅 `explicit`）。
pub fn delete_matching_text(
    conn: &Connection,
    scope_id: &str,
    chunk_text: &str,
    explicit_only: bool,
) -> Result<usize, rusqlite::Error> {
    let now = now_unix();
    let _ = delete_expired_for_scope(conn, scope_id, now)?;
    let n = if explicit_only {
        conn.execute(
            &format!(
                "DELETE FROM {TABLE} WHERE scope_id = ?1 AND chunk_text = ?2 AND source_kind = 'explicit'"
            ),
            params![scope_id, chunk_text],
        )?
    } else {
        conn.execute(
            &format!("DELETE FROM {TABLE} WHERE scope_id = ?1 AND chunk_text = ?2"),
            params![scope_id, chunk_text],
        )?
    };
    Ok(n)
}

/// 最近若干条（含 id），供 `long_term_memory_list`；从新到旧。
pub type MemoryListRow = (i64, String, String, Option<i64>, String);

pub fn list_recent_for_scope(
    conn: &Connection,
    scope_id: &str,
    limit: usize,
) -> Result<Vec<MemoryListRow>, rusqlite::Error> {
    let now = now_unix();
    let _ = delete_expired_for_scope(conn, scope_id, now)?;
    let lim = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT id, chunk_text, source_kind, expires_at_unix, tags_json FROM {TABLE} \
         WHERE scope_id = ?1 AND (expires_at_unix IS NULL OR expires_at_unix > ?3) \
         ORDER BY created_at_unix DESC LIMIT ?2"
    ))?;
    let rows = stmt.query_map(params![scope_id, lim, now], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, Option<String>>(4)?
                .unwrap_or_else(|| "[]".to_string()),
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_and_ttl_and_explicit() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let past = now_unix() - 10_000;
        insert_chunk(&conn, "s1", "auto old", "user", None, None).unwrap();
        conn.execute(
            &format!("UPDATE {TABLE} SET expires_at_unix = ?1 WHERE chunk_text = 'auto old'",),
            params![past],
        )
        .unwrap();
        let rows = list_for_scope(&conn, "s1", 10).unwrap();
        assert!(rows.is_empty());

        insert_explicit_chunk(&conn, "s1", "remember me", "[]", None, None).unwrap();
        let rows2 = list_for_scope(&conn, "s1", 10).unwrap();
        assert_eq!(rows2.len(), 1);
        assert_eq!(rows2[0].chunk_text, "remember me");

        let n = delete_matching_text(&conn, "s1", "remember me", true).unwrap();
        assert_eq!(n, 1);
    }
}
