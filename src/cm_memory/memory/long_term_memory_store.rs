//! 长期记忆：SQLite 表（可与会话库同文件或独立文件）。
//!
//! 列含可选 TTL（`expires_at_unix`）、标签 JSON、来源（`auto` / `explicit`）；过期行在读取与写入前清理。

use std::path::Path;

use log::warn;
use rusqlite::{Connection, OptionalExtension, Row, params};

const TABLE: &str = "crabmate_long_term_memory";
const META_TABLE: &str = "crabmate_long_term_memory_meta";

/// 嵌入「配方」版本：模型语义 + 文本预处理（前缀等）共同决定向量分布。
///
/// 取值一变更，历史行里的向量就与新查询向量不同分布（相似度失真），必须在 `migrate` 时作废。
/// 当前 `AllMiniLML6V2` 是普通 sentence-transformer，**不**需要 E5 式 `query:` / `passage:` 前缀。
///
/// **降级不安全**：旧二进制（不认识本版本）读到新库时不会作废旧向量，而旧代码对
/// `embedding IS NULL` 的行按 `0.0` 打分，会被 `MIN_RECALL_BASE_SCORE` 整批过滤——表现为静默零召回。
pub const EMBEDDING_RECIPE_VERSION: &str = "minilm-noprefix-v1";

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
        CREATE TABLE IF NOT EXISTS {META_TABLE} (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
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
    sync_embedding_recipe(conn)?;
    Ok(())
}

fn meta_value(conn: &Connection, key: &str) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        &format!("SELECT value FROM {META_TABLE} WHERE key = ?1"),
        params![key],
        |r| r.get(0),
    )
    .optional()
}

fn set_meta_value(conn: &Connection, key: &str, value: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        &format!("INSERT OR REPLACE INTO {META_TABLE} (key, value) VALUES (?1, ?2)"),
        params![key, value],
    )?;
    Ok(())
}

/// 嵌入配方版本与库中记录不一致时，清空全部旧向量（列置 NULL）并写回新版本。
///
/// 只清空向量、保留正文：召回侧对「无 embedding」的行回退关键词打分，
/// 因此旧条无需重嵌入也不会凭空消失，只是暂时不参与向量排序。
fn sync_embedding_recipe(conn: &Connection) -> Result<(), rusqlite::Error> {
    if meta_value(conn, "embedding_recipe")?.as_deref() == Some(EMBEDDING_RECIPE_VERSION) {
        return Ok(());
    }
    let cleared = conn.execute(
        &format!("UPDATE {TABLE} SET embedding = NULL WHERE embedding IS NOT NULL"),
        [],
    )?;
    set_meta_value(conn, "embedding_recipe", EMBEDDING_RECIPE_VERSION)?;
    if cleared > 0 {
        warn!(
            target: "crabmate",
            "长期记忆：嵌入配方更新为 {EMBEDDING_RECIPE_VERSION}，已作废 {cleared} 条旧向量（召回回退关键词）"
        );
    }
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

fn collect_query_rows<T>(
    rows: impl Iterator<Item = Result<T, rusqlite::Error>>,
) -> Result<Vec<T>, rusqlite::Error> {
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn memory_row_from_select(r: &Row<'_>) -> rusqlite::Result<MemoryRow> {
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
    let rows = stmt.query_map(params![scope_id, lim, now], memory_row_from_select)?;
    collect_query_rows(rows)
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

/// 显式记忆（`long_term_remember` / `summarize_experience` / 自动沉淀等）；`source_kind` 固定为 `explicit`。
pub fn insert_explicit_chunk(
    conn: &Connection,
    scope_id: &str,
    chunk_text: &str,
    source_role: &str,
    tags_json: &str,
    expires_at_unix: Option<i64>,
    embedding: Option<&[u8]>,
) -> Result<i64, rusqlite::Error> {
    let now = now_unix();
    let _ = delete_expired_for_scope(conn, scope_id, now)?;
    conn.execute(
        &format!(
            "INSERT INTO {TABLE} (scope_id, chunk_text, source_role, created_at_unix, expires_at_unix, tags_json, source_kind, embedding) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'explicit', ?7)"
        ),
        params![
            scope_id,
            chunk_text,
            source_role,
            now,
            expires_at_unix,
            tags_json,
            embedding
        ],
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

fn memory_list_row_from_select(r: &Row<'_>) -> rusqlite::Result<MemoryListRow> {
    Ok((
        r.get::<_, i64>(0)?,
        r.get::<_, String>(1)?,
        r.get::<_, String>(2)?,
        r.get::<_, Option<i64>>(3)?,
        r.get::<_, Option<String>>(4)?
            .unwrap_or_else(|| "[]".to_string()),
    ))
}

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
    let rows = stmt.query_map(params![scope_id, lim, now], memory_list_row_from_select)?;
    collect_query_rows(rows)
}

/// 列出该作用域内**尚无向量**的未过期行 `(id, chunk_text)`，从新到旧，供逐步回填。
///
/// 配方迁移会把历史向量整列置 NULL；若不回填，这些行将永远只能走关键词召回。
pub fn list_missing_embedding_for_scope(
    conn: &Connection,
    scope_id: &str,
    limit: usize,
) -> Result<Vec<(i64, String)>, rusqlite::Error> {
    let now = now_unix();
    let _ = delete_expired_for_scope(conn, scope_id, now)?;
    let lim = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT id, chunk_text FROM {TABLE} \
         WHERE scope_id = ?1 AND embedding IS NULL \
         AND (expires_at_unix IS NULL OR expires_at_unix > ?3) \
         ORDER BY created_at_unix DESC LIMIT ?2"
    ))?;
    let rows = stmt.query_map(params![scope_id, lim, now], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    collect_query_rows(rows)
}

/// 按主键写回向量（供配方迁移后的回填使用）。
pub fn update_embedding(
    conn: &Connection,
    id: i64,
    embedding: &[u8],
) -> Result<(), rusqlite::Error> {
    conn.execute(
        &format!("UPDATE {TABLE} SET embedding = ?1 WHERE id = ?2"),
        params![embedding, id],
    )?;
    Ok(())
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

        insert_explicit_chunk(&conn, "s1", "remember me", "explicit", "[]", None, None).unwrap();
        let rows2 = list_for_scope(&conn, "s1", 10).unwrap();
        assert_eq!(rows2.len(), 1);
        assert_eq!(rows2[0].chunk_text, "remember me");

        let n = delete_matching_text(&conn, "s1", "remember me", true).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn stale_embedding_recipe_clears_vectors_once() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        insert_chunk(&conn, "s1", "has vector", "user", None, Some(&[1, 2, 3])).unwrap();
        // 模拟库中记录的是旧配方：重新 migrate 应清空向量并写回新版本。
        set_meta_value(&conn, "embedding_recipe", "legacy-prefix-v0").unwrap();
        migrate(&conn).unwrap();
        let emb: Option<Vec<u8>> = conn
            .query_row(
                &format!("SELECT embedding FROM {TABLE} WHERE chunk_text = 'has vector'"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(emb.is_none(), "旧配方向量应被作废");
        assert_eq!(
            meta_value(&conn, "embedding_recipe").unwrap().as_deref(),
            Some(EMBEDDING_RECIPE_VERSION)
        );
        // 正文保留，仅向量被清空。
        assert_eq!(list_for_scope(&conn, "s1", 10).unwrap().len(), 1);
    }

    #[test]
    fn lists_and_updates_missing_embeddings() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        insert_chunk(&conn, "s1", "with vector", "user", None, Some(&[9, 9])).unwrap();
        insert_chunk(&conn, "s1", "no vector", "user", None, None).unwrap();
        insert_chunk(&conn, "s2", "other scope", "user", None, None).unwrap();

        let pending = list_missing_embedding_for_scope(&conn, "s1", 8).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].1, "no vector");

        update_embedding(&conn, pending[0].0, &[1, 2, 3, 4]).unwrap();
        assert!(
            list_missing_embedding_for_scope(&conn, "s1", 8)
                .unwrap()
                .is_empty()
        );
    }
}
