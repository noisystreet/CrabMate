//! 长期记忆：SQLite 表（可与会话库同文件或独立文件）。

use std::path::Path;

use rusqlite::{Connection, params};

const TABLE: &str = "crabmate_long_term_memory";

pub type MemoryRow = (i64, String, String, i64, Option<Vec<u8>>);

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

/// 列顺序：id, chunk_text, source_role, created_at_unix, embedding
pub fn list_for_scope(
    conn: &Connection,
    scope_id: &str,
    limit: usize,
) -> Result<Vec<MemoryRow>, rusqlite::Error> {
    let lim = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT id, chunk_text, source_role, created_at_unix, embedding FROM {TABLE} WHERE scope_id = ?1 ORDER BY created_at_unix DESC LIMIT ?2"
    ))?;
    let rows = stmt.query_map(params![scope_id, lim], |r| {
        let emb: Option<Vec<u8>> = r.get(4)?;
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            emb,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn insert_chunk(
    conn: &Connection,
    scope_id: &str,
    chunk_text: &str,
    source_role: &str,
    embedding: Option<&[u8]>,
) -> Result<(), rusqlite::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    conn.execute(
        &format!(
            "INSERT INTO {TABLE} (scope_id, chunk_text, source_role, created_at_unix, embedding) VALUES (?1, ?2, ?3, ?4, ?5)"
        ),
        params![scope_id, chunk_text, source_role, now, embedding],
    )?;
    Ok(())
}

pub fn delete_oldest_beyond(
    conn: &Connection,
    scope_id: &str,
    keep: usize,
) -> Result<(), rusqlite::Error> {
    let keep = i64::try_from(keep).unwrap_or(i64::MAX);
    conn.execute(
        &format!(
            "DELETE FROM {TABLE} WHERE scope_id = ?1 AND id NOT IN (
                SELECT id FROM {TABLE} WHERE scope_id = ?1 ORDER BY created_at_unix DESC LIMIT ?2
            )"
        ),
        params![scope_id, keep],
    )?;
    Ok(())
}

/// 若存在相同 `chunk_text` 的行则跳过插入（简单去重）。
pub fn has_duplicate_text(
    conn: &Connection,
    scope_id: &str,
    chunk_text: &str,
) -> Result<bool, rusqlite::Error> {
    let n: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM {TABLE} WHERE scope_id = ?1 AND chunk_text = ?2"),
        params![scope_id, chunk_text],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}
