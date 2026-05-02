//! **`im.message.receive_v1`** 的 **SQLite 持久化队列**（与内存 `mpsc` 二选一）。
//!
//! 飞书在收到 **HTTP 200** 后可能仍重试；业务层已有 **`message_id`** 去重。队列侧仍可能收到重复入队，故 **`enqueue`** 使用 **`INSERT OR IGNORE`** 与 **`message_id` + `event_id`** 派生的唯一键去重。

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use tracing::warn;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS feishu_im_queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    dedupe_key TEXT NOT NULL,
    envelope TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at INTEGER NOT NULL,
    lease_expires_at INTEGER
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_feishu_im_queue_dedupe
    ON feishu_im_queue(dedupe_key);
CREATE INDEX IF NOT EXISTS idx_feishu_im_queue_pending
    ON feishu_im_queue(status, id);
"#;

fn extract_dedupe_key(v: &Value) -> String {
    let mid = v
        .pointer("/event/message/message_id")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    let eid = v
        .pointer("/header/event_id")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("uuid").and_then(|x| x.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    if !eid.is_empty() {
        format!("eid:{eid}")
    } else if !mid.is_empty() {
        format!("mid:{mid}")
    } else {
        format!("anon:{}", uuid_placeholder(v))
    }
}

fn uuid_placeholder(v: &Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let s = v.to_string();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// 打开或创建队列库，并执行建表迁移。
pub fn open_queue(path: &Path) -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;\nPRAGMA foreign_keys=ON;\nPRAGMA busy_timeout=5000;\n",
    )?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

#[derive(Clone)]
pub struct FeishuImEventSqliteQueue {
    inner: Arc<Mutex<Connection>>,
    max_retries: u32,
    poll_idle_ms: u64,
}

impl FeishuImEventSqliteQueue {
    pub fn new(path: &Path, max_retries: u32, poll_idle_ms: u64) -> Result<Self, rusqlite::Error> {
        let conn = open_queue(path)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(conn)),
            max_retries: max_retries.max(1),
            poll_idle_ms: poll_idle_ms.max(50),
        })
    }

    /// 入队；**`dedupe_key` 冲突时忽略**（幂等）。
    pub fn enqueue(&self, envelope: &Value) -> Result<(), rusqlite::Error> {
        let dedupe_key = extract_dedupe_key(envelope);
        let json = serde_json::to_string(envelope).map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(
                Box::new(e) as Box<dyn std::error::Error + Send + Sync>
            )
        })?;
        let now = unix_secs();
        let g = self.inner.lock().expect("sqlite queue mutex poisoned");
        g.execute(
            "INSERT OR IGNORE INTO feishu_im_queue (dedupe_key, envelope, status, attempts, created_at)
             VALUES (?1, ?2, 'pending', 0, ?3)",
            rusqlite::params![dedupe_key, json, now],
        )?;
        Ok(())
    }

    /// 将过期租约收回为 **`pending`**，便于崩溃恢复。
    pub fn reclaim_expired(&self, now_secs: i64) -> Result<usize, rusqlite::Error> {
        let g = self.inner.lock().expect("sqlite queue mutex poisoned");
        let n = g.execute(
            "UPDATE feishu_im_queue SET status = 'pending', lease_expires_at = NULL
             WHERE status = 'processing' AND lease_expires_at IS NOT NULL AND lease_expires_at < ?1",
            [now_secs],
        )?;
        Ok(n)
    }

    /// 认领一条 **`pending`**。返回 **`(id, envelope_json)`**。
    pub fn claim_one(
        &self,
        now_secs: i64,
        lease_secs: i64,
    ) -> Result<Option<(i64, String)>, rusqlite::Error> {
        let lease_until = now_secs.saturating_add(lease_secs.max(30));
        let mut g = self.inner.lock().expect("sqlite queue mutex poisoned");
        let tx = g.transaction()?;
        let id: Option<i64> = tx
            .query_row(
                "SELECT id FROM feishu_im_queue WHERE status = 'pending' ORDER BY id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let Some(id) = id else {
            tx.commit()?;
            return Ok(None);
        };
        let env: String = tx.query_row(
            "SELECT envelope FROM feishu_im_queue WHERE id = ?1",
            [id],
            |r| r.get(0),
        )?;
        tx.execute(
            "UPDATE feishu_im_queue SET status = 'processing', lease_expires_at = ?1 WHERE id = ?2",
            rusqlite::params![lease_until, id],
        )?;
        tx.commit()?;
        Ok(Some((id, env)))
    }

    pub fn mark_done(&self, id: i64) -> Result<(), rusqlite::Error> {
        let g = self.inner.lock().expect("sqlite queue mutex poisoned");
        g.execute("DELETE FROM feishu_im_queue WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn mark_retry_or_fail(&self, id: i64, err: &str) -> Result<(), rusqlite::Error> {
        let preview = clip_error(err, 2000);
        let g = self.inner.lock().expect("sqlite queue mutex poisoned");
        let attempts: i64 = g.query_row(
            "SELECT attempts FROM feishu_im_queue WHERE id = ?1",
            [id],
            |r| r.get(0),
        )?;
        let next = attempts.saturating_add(1);
        if next >= i64::from(self.max_retries) {
            g.execute(
                "UPDATE feishu_im_queue SET status = 'failed', last_error = ?1, lease_expires_at = NULL WHERE id = ?2",
                rusqlite::params![preview, id],
            )?;
            warn!(
                queue_id = id,
                attempts = next,
                "feishu sqlite queue item failed permanently"
            );
        } else {
            g.execute(
                "UPDATE feishu_im_queue SET status = 'pending', attempts = ?1, last_error = ?2, lease_expires_at = NULL WHERE id = ?3",
                rusqlite::params![next, preview, id],
            )?;
        }
        Ok(())
    }

    pub fn poll_idle_ms(&self) -> u64 {
        self.poll_idle_ms
    }
}

fn clip_error(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        t.to_string()
    } else {
        t.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    }
}

fn unix_secs() -> i64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dedupe_inserts_single_row_per_event_id() {
        let v = json!({
            "header": { "event_id": "evt_1" },
            "event": { "message": { "message_id": "om_x" } }
        });
        let db_path =
            std::env::temp_dir().join(format!("feishu_im_q_test_{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(format!("{}-wal", db_path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", db_path.display()));
        {
            let q = FeishuImEventSqliteQueue::new(db_path.as_path(), 3, 50).unwrap();
            q.enqueue(&v).unwrap();
            q.enqueue(&v).unwrap();
        }
        let c = open_queue(db_path.as_path()).unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM feishu_im_queue", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        drop(c);
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(format!("{}-wal", db_path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", db_path.display()));
    }
}
