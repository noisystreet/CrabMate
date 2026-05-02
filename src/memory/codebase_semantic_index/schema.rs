//! SQLite 表结构常量、migrate、打开索引路径。

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use crate::tools::canonical_workspace_root;

pub(crate) const TABLE: &str = "crabmate_codebase_chunks";
pub(crate) const TABLE_FILES: &str = "crabmate_codebase_files";
/// FTS5 虚拟表（`content=` 指向 [`TABLE`]，rowid = `id`）。
pub(crate) const TABLE_FTS: &str = "crabmate_codebase_chunks_fts";
/// 供失效逻辑删除文件目录表（与 chunks 同步）。
pub(crate) const CODEBASE_SEMANTIC_FILES_TABLE: &str = TABLE_FILES;
pub(crate) const SCHEMA_VERSION: i64 = 4;

fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(&format!(
        r#"
        CREATE TABLE IF NOT EXISTS crabmate_codebase_index_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS {TABLE} (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_root TEXT NOT NULL,
            rel_path TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            chunk_text TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            embedding BLOB NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_{TABLE}_workspace ON {TABLE}(workspace_root);
        CREATE INDEX IF NOT EXISTS idx_{TABLE}_ws_rel ON {TABLE}(workspace_root, rel_path);
        CREATE TABLE IF NOT EXISTS {TABLE_FILES} (
            workspace_root TEXT NOT NULL,
            rel_path TEXT NOT NULL,
            size INTEGER NOT NULL,
            mtime_ns INTEGER NOT NULL,
            content_sha256 TEXT NOT NULL,
            PRIMARY KEY (workspace_root, rel_path)
        );
        CREATE INDEX IF NOT EXISTS idx_{TABLE_FILES}_ws ON {TABLE_FILES}(workspace_root);
        "#
    ))?;

    let ver: Option<i64> = conn
        .query_row(
            "SELECT value FROM crabmate_codebase_index_meta WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .and_then(|s| s.parse().ok());

    let prev = ver.unwrap_or(0);
    if prev < SCHEMA_VERSION {
        let _ = conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_{TABLE}_ws_rel ON {TABLE}(workspace_root, rel_path)"
            ),
            [],
        );
        let _ = conn.execute_batch(&format!(
            r#"
            CREATE TABLE IF NOT EXISTS {TABLE_FILES} (
                workspace_root TEXT NOT NULL,
                rel_path TEXT NOT NULL,
                size INTEGER NOT NULL,
                mtime_ns INTEGER NOT NULL,
                content_sha256 TEXT NOT NULL,
                PRIMARY KEY (workspace_root, rel_path)
            );
            CREATE INDEX IF NOT EXISTS idx_{TABLE_FILES}_ws ON {TABLE_FILES}(workspace_root);
            "#
        ));
    }

    // FTS5 外挂块表（rowid = chunk id）；触发器保持与 INSERT/UPDATE/DELETE 同步。
    conn.execute_batch(&format!(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS {TABLE_FTS} USING fts5(
            chunk_text,
            content='{TABLE}',
            content_rowid='id',
            tokenize = 'unicode61 remove_diacritics 2'
        );
        CREATE TRIGGER IF NOT EXISTS crabmate_codebase_chunks_ai AFTER INSERT ON {TABLE} BEGIN
            INSERT INTO {TABLE_FTS}(rowid, chunk_text) VALUES (new.id, new.chunk_text);
        END;
        CREATE TRIGGER IF NOT EXISTS crabmate_codebase_chunks_ad AFTER DELETE ON {TABLE} BEGIN
            INSERT INTO {TABLE_FTS}(crabmate_codebase_chunks_fts, rowid) VALUES('delete', old.id);
        END;
        CREATE TRIGGER IF NOT EXISTS crabmate_codebase_chunks_au AFTER UPDATE ON {TABLE} BEGIN
            INSERT INTO {TABLE_FTS}(crabmate_codebase_chunks_fts, rowid) VALUES('delete', old.id);
            INSERT INTO {TABLE_FTS}(rowid, chunk_text) VALUES (new.id, new.chunk_text);
        END;
        "#
    ))?;

    if prev < 4 {
        // 从旧版升级或首次启用 FTS：用 content 表全量回填全文索引。
        let _ = conn.execute(
            &format!("INSERT INTO {TABLE_FTS}({TABLE_FTS}) VALUES('rebuild')"),
            [],
        );
    }

    conn.execute(
        "INSERT OR REPLACE INTO crabmate_codebase_index_meta (key, value) VALUES ('schema_version', ?1)",
        params![SCHEMA_VERSION.to_string()],
    )?;

    Ok(())
}

/// 打开或创建索引库并迁移 schema（不写日志全文）。
pub(crate) fn open_codebase_semantic_db(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("无法创建索引目录 {}: {}", parent.display(), e))?;
    }
    let conn = Connection::open(path)
        .map_err(|e| format!("无法打开代码语义索引库 {}: {}", path.display(), e))?;
    migrate(&conn).map_err(|e| format!("代码语义索引 schema 初始化失败: {}", e))?;
    Ok(conn)
}

pub(crate) fn index_path_for_workspace(
    workspace_root: &Path,
    configured: &str,
) -> Result<PathBuf, String> {
    let base = canonical_workspace_root(workspace_root).map_err(|e| e.user_message())?;
    if configured.trim().is_empty() {
        return Ok(base.join(".crabmate/codebase_semantic.sqlite"));
    }
    let sub = configured.trim();
    if Path::new(sub).is_absolute() {
        return Err("codebase_semantic_index_sqlite_path 必须为相对工作区的相对路径".to_string());
    }
    let joined = base.join(sub);
    let canon = joined
        .canonicalize()
        .map_err(|e| format!("索引路径无法解析: {}", e))?;
    if !canon.starts_with(&base) {
        return Err("索引路径不能超出工作区根目录".to_string());
    }
    Ok(canon)
}
