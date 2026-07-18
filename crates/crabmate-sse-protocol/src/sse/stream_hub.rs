//! `/chat/stream` 进程内 **SSE 断线重连**：`broadcast` 多订户 + 环形缓冲 + **`Last-Event-ID`** / **`stream_resume`**。
//!
//! 每条逻辑事件为 **`id: <seq>\ndata: <payload>\n\n`**（`payload` 与历史一致：单行 JSON 或纯文本 delta）。
//!
//! 设置 `CM_SSE_REPLAY_DUMP_DIR` 后，所有 SSE 事件额外追加写入 `sse-replay-events.jsonl`，
//! 供前端 TurnLayout replay 调试使用。

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use super::protocol::SSE_RESUME_RING_CAP;

/// SSE replay dump 文件名。
const SSE_REPLAY_FILE: &str = "sse-replay-events.jsonl";

static SSE_REPLAY_DUMP_DIR_LOGGED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

fn sse_replay_dump_dir() -> Option<std::path::PathBuf> {
    let s = std::env::var_os("CM_SSE_REPLAY_DUMP_DIR")?;
    let t = s.to_string_lossy();
    let trimmed = t.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(trimmed))
    }
}

/// 将 SSE 事件 payload 追加写入 replay JSONL 文件。
fn append_sse_replay_line(job_id: u64, seq: u64, data_payload: &str) {
    let Some(dir) = sse_replay_dump_dir() else {
        return;
    };
    if SSE_REPLAY_DUMP_DIR_LOGGED.get().is_none() {
        log::info!(
            target: "crabmate",
            "SSE replay dump enabled: {}",
            dir.display()
        );
        let _ = SSE_REPLAY_DUMP_DIR_LOGGED.set(());
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!(
            target: "crabmate",
            "SSE replay dump: create_dir_all {:?} failed: {e}",
            dir
        );
        return;
    }
    let path = dir.join(SSE_REPLAY_FILE);
    let line = serde_json::json!({
        "seq": seq,
        "job_id": job_id,
        "data": data_payload,
    });
    let mut out = serde_json::to_string(&line).unwrap_or_default();
    out.push('\n');
    if let Err(e) = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        f.write_all(out.as_bytes())?;
        f.flush()?;
        Ok(())
    })() {
        log::warn!(
            target: "crabmate",
            "SSE replay dump: append {} failed: {e}",
            path.display()
        );
    }
}

#[derive(Debug)]
struct StreamEntry {
    next_seq: AtomicU64,
    ring: Mutex<VecDeque<(u64, String)>>,
    tx: broadcast::Sender<(u64, String)>,
}

/// 单进程、单 `serve` 实例：按 `job_id` 索引活跃流式任务。
#[derive(Clone, Default)]
pub struct SseStreamHub {
    inner: Arc<Mutex<HashMap<u64, Arc<StreamEntry>>>>,
}

impl SseStreamHub {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 为新 `job_id` 注册 hub；若已存在则返回现有（幂等）。
    pub fn register_job(&self, job_id: u64) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.entry(job_id).or_insert_with(|| {
            let (tx, _) =
                broadcast::channel::<(u64, String)>(SSE_RESUME_RING_CAP.saturating_mul(2).max(64));
            Arc::new(StreamEntry {
                next_seq: AtomicU64::new(0),
                ring: Mutex::new(VecDeque::new()),
                tx,
            })
        });
    }

    fn entry(&self, job_id: u64) -> Option<Arc<StreamEntry>> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&job_id)
            .cloned()
    }

    /// 递增序号、写入环形缓冲、向所有订户广播（**含** `id:` / `data:` / 空行）。
    /// 返回 **`(seq, data_payload)`** 供 HTTP 层设置 `Event::id` / `data`；`job_id` 未注册时返回 `None`。
    ///
    /// 当 `CM_SSE_REPLAY_DUMP_DIR` 设置时，同时将事件追加写入 `sse-replay-events.jsonl`。
    pub fn publish(&self, job_id: u64, data_payload: String) -> Option<(u64, String)> {
        let e = self.entry(job_id)?;
        let seq = e.next_seq.fetch_add(1, Ordering::SeqCst) + 1;
        {
            let mut ring = e.ring.lock().unwrap_or_else(|e| e.into_inner());
            ring.push_back((seq, data_payload.clone()));
            while ring.len() > SSE_RESUME_RING_CAP {
                ring.pop_front();
            }
        }
        let _ = e.tx.send((seq, data_payload.clone()));
        append_sse_replay_line(job_id, seq, &data_payload);
        Some((seq, data_payload))
    }

    /// 订阅实时事件（`recv()` 得到 **`(seq, data 负载)`**）。
    pub fn subscribe(&self, job_id: u64) -> Option<broadcast::Receiver<(u64, String)>> {
        self.entry(job_id).map(|e| e.tx.subscribe())
    }

    /// 环形缓冲中 `seq > after_seq` 的 `data:` 负载（不含 `id` 行），按序。
    pub fn replay_after(&self, job_id: u64, after_seq: u64) -> Option<Vec<(u64, String)>> {
        let e = self.entry(job_id)?;
        let ring = e.ring.lock().unwrap_or_else(|e| e.into_inner());
        Some(
            ring.iter()
                .filter(|(s, _)| *s > after_seq)
                .cloned()
                .collect(),
        )
    }

    pub fn remove_job(&self, job_id: u64) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.remove(&job_id);
    }

    pub fn has_job(&self, job_id: u64) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&job_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_trims_and_replay() {
        let hub = SseStreamHub::new();
        hub.register_job(7);
        for i in 0..10 {
            let _ = hub.publish(7, format!("m{i}"));
        }
        let r = hub.replay_after(7, 0).expect("replay");
        assert_eq!(r.len(), 10);
        assert_eq!(r[0].0, 1);
        assert_eq!(r[9].0, 10);
    }

    static REPLAY_DUMP_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn sse_replay_dump_writes_jsonl_when_env_set() {
        let _lock = REPLAY_DUMP_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        // SAFETY: REPLAY_DUMP_TEST_LOCK serializes env var access.
        unsafe {
            std::env::set_var("CM_SSE_REPLAY_DUMP_DIR", dir.path().as_os_str());
        }
        let hub = SseStreamHub::new();
        hub.register_job(42);
        let _ = hub.publish(42, r#"{"type":"CUSTOM","customType":"timeline_log","data":{"kind":"final_response","title":"终答"}} "#.to_string());
        let _ = hub.publish(42, "plain text delta".to_string());
        unsafe {
            std::env::remove_var("CM_SSE_REPLAY_DUMP_DIR");
        }
        let path = dir.path().join(SSE_REPLAY_FILE);
        let raw = std::fs::read_to_string(&path).expect("jsonl should exist");
        let lines: Vec<&str> = raw.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 2, "should have 2 lines, got: {raw}");
        let v1: serde_json::Value = serde_json::from_str(lines[0]).expect("line 1 json");
        assert_eq!(v1["seq"], 1);
        assert_eq!(v1["job_id"], 42);
        assert!(v1["data"].as_str().unwrap().contains("timeline_log"));
        let v2: serde_json::Value = serde_json::from_str(lines[1]).expect("line 2 json");
        assert_eq!(v2["seq"], 2);
        assert_eq!(v2["data"], "plain text delta");
    }

    #[test]
    fn sse_replay_dump_no_file_when_env_unset() {
        let _lock = REPLAY_DUMP_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        // 确保环境变量未设置
        unsafe {
            std::env::remove_var("CM_SSE_REPLAY_DUMP_DIR");
        }
        let hub = SseStreamHub::new();
        hub.register_job(99);
        let _ = hub.publish(99, "no dump".to_string());
        let path = dir.path().join(SSE_REPLAY_FILE);
        assert!(!path.exists(), "jsonl should not exist when env unset");
    }
}
