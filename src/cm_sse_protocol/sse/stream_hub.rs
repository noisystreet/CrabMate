//! `/chat/stream` 进程内 **SSE 断线重连**：`broadcast` 多订户 + 环形缓冲 + **`Last-Event-ID`** / **`stream_resume`**。
//!
//! 每条逻辑事件为 **`id: <seq>\ndata: <payload>\n\n`**（`payload` 与历史一致：单行 JSON 或纯文本 delta）。
//!
//! 设置 `CM_SSE_REPLAY_DUMP_DIR` 后，所有 SSE 事件额外追加写入 `sse-replay-events.jsonl`，
//! 供前端 TurnLayout replay 调试使用。
//!
//! ## 生命周期：登记跟着「还能不能产帧」走（#1316）
//!
//! 这里只承认**一个**事实：`sse_tx` 的所有克隆都 drop ⟺ 桥接任务的 `recv` 返回 `None`
//! ⟺ 这条流不可能再产出新帧。map 因此只存 **`Weak<StreamEntry>`**，强引用由
//! [`StreamPublisher`] 持有，桥接任务退出时 [`StreamPublisher::finalize`] 摘除登记；
//! 于是「登记还在」同时等价于「还可能产帧」与「还能 `stream_resume`」。
//!
//! 投递**不查 map**（[`StreamPublisher::publish`] 恒成功）。旧实现让 worker 的
//! `remove_job` 去代表「不会再有帧」：worker 一返回就把登记删掉，而落盘后连发的
//! `conversation_saved` / state snapshot / `RUN_FINISHED` 还排在 channel 里、桥接任务
//! 尚未被调度，查表投递只能拿到 `None`，终态帧被静默丢弃（客户端只剩轮末补拉可用）。
//!
//! **依赖的不变式**：`sse_tx` 的克隆不得跨回合存活——后台工具任务的终态补发用
//! `out_tx().downgrade()` 正是为此。若哪天有克隆被长期持有，桥接任务与登记都会跟着长存
//! （改动前同样如此，只是登记会先被 worker 删掉，帧照样丢）。

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

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

/// 「还会产帧」的登记项：序号源 + 环形缓冲 + 广播发送端。
///
/// 存活期**严格等于**持有它的 [`StreamPublisher`] 的存活期（hub map 只存 `Weak`）：
/// 「登记还在」遂等价于「这条流还可能产出新帧」，不由任何调用方在某个时刻去猜。
#[derive(Debug)]
struct StreamEntry {
    next_seq: AtomicU64,
    ring: Mutex<VecDeque<(u64, String)>>,
    tx: broadcast::Sender<(u64, String)>,
}

impl StreamEntry {
    fn new() -> Self {
        let (tx, _) =
            broadcast::channel::<(u64, String)>(SSE_RESUME_RING_CAP.saturating_mul(2).max(64));
        Self {
            next_seq: AtomicU64::new(0),
            ring: Mutex::new(VecDeque::new()),
            tx,
        }
    }

    /// 递增序号、写入环形缓冲、向所有订户广播（**含** `id:` / `data:` / 空行）。
    /// 返回 **`(seq, data_payload)`** 供 HTTP 层设置 `Event::id` / `data`。
    ///
    /// 当 `CM_SSE_REPLAY_DUMP_DIR` 设置时，同时将事件追加写入 `sse-replay-events.jsonl`。
    fn publish(&self, job_id: u64, data_payload: String) -> (u64, String) {
        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst) + 1;
        {
            let mut ring = self.ring.lock().unwrap_or_else(|e| e.into_inner());
            ring.push_back((seq, data_payload.clone()));
            while ring.len() > SSE_RESUME_RING_CAP {
                ring.pop_front();
            }
        }
        // 无订户时返回 `Err`，属正常（重连靠环形缓冲补）。
        let _ = self.tx.send((seq, data_payload.clone()));
        append_sse_replay_line(job_id, seq, &data_payload);
        (seq, data_payload)
    }

    /// 环形缓冲中 `seq > after_seq` 的 `data:` 负载（不含 `id` 行），按序。
    fn replay_after(&self, after_seq: u64) -> Vec<(u64, String)> {
        let ring = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        ring.iter()
            .filter(|(s, _)| *s > after_seq)
            .cloned()
            .collect()
    }
}

/// `job_id` → 存活登记项。**弱引用**：存活期由 [`StreamPublisher`] 决定，见模块文档。
type HubMap = Mutex<HashMap<u64, Weak<StreamEntry>>>;

/// 单进程、单 `serve` 实例：按 `job_id` 索引活跃流式任务。
#[derive(Clone, Default)]
pub struct SseStreamHub {
    inner: Arc<HubMap>,
}

impl SseStreamHub {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 为 `job_id` 登记投递句柄；已有**存活**登记时复用同一序号源（幂等）。
    ///
    /// 返回的 [`StreamPublisher`] 是登记项的强引用：**drop 它即等于注销**（弱引用随即失效）。
    /// 调用方必须把它交给桥接任务，而不是就地丢弃。
    pub fn register_job(&self, job_id: u64) -> StreamPublisher {
        let entry = {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            // 顺手清掉已注销（弱引用失效）的键：map 大小严格跟随存活流数量。
            g.retain(|_, w| w.strong_count() > 0);
            match g.get(&job_id).and_then(Weak::upgrade) {
                Some(existing) => {
                    log::warn!(
                        target: "crabmate",
                        "chat stream job_id={} 重复登记且旧登记仍存活：沿用旧序号源（job_id 不应复用）",
                        job_id
                    );
                    existing
                }
                None => {
                    let fresh = Arc::new(StreamEntry::new());
                    g.insert(job_id, Arc::downgrade(&fresh));
                    fresh
                }
            }
        };
        StreamPublisher {
            job_id,
            entry,
            hub: Arc::downgrade(&self.inner),
        }
    }

    /// 存活登记——唯一判据：登记项还在 ⟺ 这条流还可能产帧 ⟺ 仍受理 `stream_resume`。
    fn live_entry(&self, job_id: u64) -> Option<Arc<StreamEntry>> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&job_id)
            .and_then(Weak::upgrade)
    }

    /// 查表投递：无存活登记时返回 `None` 并留 debug 日志。
    ///
    /// **只供测试与一次性代码**；桥接任务必须用 [`StreamPublisher::publish`]——查表版在
    /// 「登记已摘除但帧还在队列里」时返回 `None`，正是 #1316 的丢帧路径。
    pub fn publish(&self, job_id: u64, data_payload: String) -> Option<(u64, String)> {
        match self.live_entry(job_id) {
            Some(entry) => Some(entry.publish(job_id, data_payload)),
            None => {
                log::debug!(
                    target: "crabmate",
                    "chat stream 投递时 job_id={} 已无存活登记，帧被丢弃（投递应走 StreamPublisher）",
                    job_id
                );
                None
            }
        }
    }

    /// 订阅实时事件（`recv()` 得到 **`(seq, data 负载)`**）。
    pub fn subscribe(&self, job_id: u64) -> Option<broadcast::Receiver<(u64, String)>> {
        self.live_entry(job_id).map(|e| e.tx.subscribe())
    }

    /// 环形缓冲中 `seq > after_seq` 的 `data:` 负载（不含 `id` 行），按序。
    pub fn replay_after(&self, job_id: u64, after_seq: u64) -> Option<Vec<(u64, String)>> {
        self.live_entry(job_id).map(|e| e.replay_after(after_seq))
    }

    /// 摘除登记。桥接任务退出时走 [`StreamPublisher::finalize`]（带「是否仍是自己」校验）。
    pub fn remove_job(&self, job_id: u64) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.remove(&job_id);
    }

    pub fn has_job(&self, job_id: u64) -> bool {
        self.live_entry(job_id).is_some()
    }
}

/// 已登记 `job_id` 的**投递句柄**。
///
/// 持有登记项（`StreamEntry`，私有）的强引用，所以 [`Self::publish`] 不查 hub map、恒成功：
/// worker 收尾（`drop(sse_tx)` → 返回）与桥接任务被调度之间的窗口里，已入队的终态帧仍能
/// 送达 HTTP 侧（#1316）。
#[derive(Clone)]
pub struct StreamPublisher {
    job_id: u64,
    entry: Arc<StreamEntry>,
    /// 仅供 [`Self::finalize`] 摘除自己的登记；持 `Weak` 以免 `hub → entry → hub` 成环。
    hub: Weak<HubMap>,
}

impl StreamPublisher {
    pub fn job_id(&self) -> u64 {
        self.job_id
    }

    /// 递增序号、写入环形缓冲、广播；**恒成功**（无订户时 `send` 的 `Err` 属正常）。
    pub fn publish(&self, data_payload: String) -> (u64, String) {
        self.entry.publish(self.job_id, data_payload)
    }

    /// 桥接任务退出（`sse_rx` 关闭 ⟺ 本流不可能再产帧）时摘除登记。
    ///
    /// 只摘除仍指向**本**登记项的那个键：同 `job_id` 已被重新登记时不误删新的。
    pub fn finalize(&self) {
        let Some(inner) = self.hub.upgrade() else {
            return;
        };
        let mut g = inner.lock().unwrap_or_else(|e| e.into_inner());
        let still_self = g
            .get(&self.job_id)
            .and_then(Weak::upgrade)
            .is_some_and(|e| Arc::ptr_eq(&e, &self.entry));
        if still_self {
            g.remove(&self.job_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_trims_and_replay() {
        let hub = SseStreamHub::new();
        let publisher = hub.register_job(7);
        for i in 0..10 {
            let _ = publisher.publish(format!("m{i}"));
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
        let _publisher = hub.register_job(42);
        let _ = hub.publish(42, r#"{"type":"CUSTOM","customType":"timeline_log","data":{"kind":"final_response","title":"终答"}} "#.to_string());
        let _ = hub.publish(42, "plain text delta".to_string());
        unsafe {
            std::env::remove_var("CM_SSE_REPLAY_DUMP_DIR");
        }
        let path = dir.path().join(SSE_REPLAY_FILE);
        let raw = std::fs::read_to_string(&path).expect("jsonl should exist");
        // `CM_SSE_REPLAY_DUMP_DIR` 是进程级环境变量：本用例设它期间，**任何**并发测试
        // 线程的 `publish` 都会落到同一个 dump 文件（例如同文件 `replay_after` 的 job 7）。
        // 锁只序列化「读写 env 的测试之间」，挡不住不碰 env 却在窗口内 publish 的测试，
        // 因此不能靠行数断言，只挑本用例的 `job_id`。
        let mine: Vec<serde_json::Value> = raw
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .filter(|v: &serde_json::Value| v["job_id"].as_u64() == Some(42))
            .collect();
        assert_eq!(mine.len(), 2, "should have 2 lines for job 42, got: {raw}");
        let v1 = &mine[0];
        assert_eq!(v1["seq"], 1);
        assert_eq!(v1["job_id"], 42);
        assert!(v1["data"].as_str().unwrap().contains("timeline_log"));
        let v2 = &mine[1];
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
        let _publisher = hub.register_job(99);
        let _ = hub.publish(99, "no dump".to_string());
        let path = dir.path().join(SSE_REPLAY_FILE);
        assert!(!path.exists(), "jsonl should not exist when env unset");
    }

    /// #1316 最小复现：登记被摘除后，句柄仍须能把已入队帧投递出去。
    #[test]
    fn publisher_publishes_after_remove_job() {
        let hub = SseStreamHub::new();
        let publisher = hub.register_job(7);
        let mut sub = hub.subscribe(7).expect("subscribe");
        hub.remove_job(7);

        assert!(!hub.has_job(7), "摘除后不再受理重连");
        assert!(hub.replay_after(7, 0).is_none(), "摘除后不可重放");
        assert!(
            hub.publish(7, "late".to_string()).is_none(),
            "查表投递失效——这正是旧实现在收尾窗口里丢终态帧的路径"
        );

        let (seq, data) = publisher.publish("terminal".to_string());
        assert_eq!((seq, data.as_str()), (1, "terminal"));
        assert_eq!(
            sub.try_recv().expect("late frame"),
            (1, "terminal".to_string())
        );
    }

    /// 登记项随最后一个 `StreamPublisher` 消失：调用方不再需要 `remove_job`。
    #[test]
    fn entry_dies_with_last_publisher() {
        let hub = SseStreamHub::new();
        let publisher = hub.register_job(8);
        assert!(hub.has_job(8));
        drop(publisher);
        assert!(!hub.has_job(8), "句柄 drop 即注销");
        assert!(hub.publish(8, "x".to_string()).is_none());
        assert!(hub.replay_after(8, 0).is_none());
    }

    /// `finalize` 幂等，且不误删同 `job_id` 的后来登记。
    #[test]
    fn finalize_removes_own_entry_only() {
        let hub = SseStreamHub::new();
        let publisher = hub.register_job(11);
        publisher.finalize();
        assert!(!hub.has_job(11));
        publisher.finalize();

        let again = hub.register_job(11);
        assert!(hub.has_job(11), "重新登记后应可重连");
        publisher.finalize();
        assert!(hub.has_job(11), "旧句柄的 finalize 不应摘除新登记");
        drop(again);
    }

    /// 失效键在下次登记时清理：map 大小严格跟随存活流数量。
    #[test]
    fn register_prunes_dead_entries() {
        let hub = SseStreamHub::new();
        drop(hub.register_job(1));
        drop(hub.register_job(2));
        let alive = hub.register_job(3);
        assert!(!hub.has_job(1));
        assert!(hub.has_job(3));
        assert_eq!(
            hub.inner.lock().unwrap().len(),
            1,
            "失效键应已被清理，只留存活登记"
        );
        drop(alive);
    }
}
