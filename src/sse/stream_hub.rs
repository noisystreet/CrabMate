//! `/chat/stream` 进程内 **SSE 断线重连**：`broadcast` 多订户 + 环形缓冲 + **`Last-Event-ID`** / **`stream_resume`**。
//!
//! 每条逻辑事件为 **`id: <seq>\ndata: <payload>\n\n`**（`payload` 与历史一致：单行 JSON 或纯文本 delta）。

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use super::protocol::SSE_RESUME_RING_CAP;

#[derive(Debug)]
struct StreamEntry {
    next_seq: AtomicU64,
    ring: Mutex<VecDeque<(u64, String)>>,
    tx: broadcast::Sender<(u64, String)>,
}

/// 单进程、单 `serve` 实例：按 `job_id` 索引活跃流式任务。
#[derive(Clone, Default)]
pub(crate) struct SseStreamHub {
    inner: Arc<Mutex<HashMap<u64, Arc<StreamEntry>>>>,
}

impl SseStreamHub {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 为新 `job_id` 注册 hub；若已存在则返回现有（幂等）。
    pub(crate) fn register_job(&self, job_id: u64) {
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
    pub(crate) fn publish(&self, job_id: u64, data_payload: String) -> Option<(u64, String)> {
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
        Some((seq, data_payload))
    }

    /// 订阅实时事件（`recv()` 得到 **`(seq, data 负载)`**）。
    pub(crate) fn subscribe(&self, job_id: u64) -> Option<broadcast::Receiver<(u64, String)>> {
        self.entry(job_id).map(|e| e.tx.subscribe())
    }

    /// 环形缓冲中 `seq > after_seq` 的 `data:` 负载（不含 `id` 行），按序。
    pub(crate) fn replay_after(&self, job_id: u64, after_seq: u64) -> Option<Vec<(u64, String)>> {
        let e = self.entry(job_id)?;
        let ring = e.ring.lock().unwrap_or_else(|e| e.into_inner());
        Some(
            ring.iter()
                .filter(|(s, _)| *s > after_seq)
                .cloned()
                .collect(),
        )
    }

    pub(crate) fn remove_job(&self, job_id: u64) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.remove(&job_id);
    }

    pub(crate) fn has_job(&self, job_id: u64) -> bool {
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
}
