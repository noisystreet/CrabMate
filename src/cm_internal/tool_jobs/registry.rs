//! 后台任务注册表：进程内 `Mutex<HashMap>`，队列/并发/条目限额、TTL 清理、取消/完成转移。
//!
//! - **单副本**：内存态，serve 重启即丢（启动 sweep 为空操作，本模块文档明示不承诺崩溃恢复）。
//! - **多副本**：需外部代理/持久化，另立项。
//! - `tool_job_id` 用 [`getrandom`] 生成 16 随机字节（32 hex），不可枚举 → 知晓 id 即能力凭证。

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::cm_tools::subprocess_session::SessionStream;

use super::types::{
    JobFinishedSink, JobLimits, JobOutcome, JobOutputLog, JobRecord, JobSpawn, JobStatus,
    OutputLogRead,
};

/// 进程内注册表（所有操作持锁，单临界区保证状态转移原子性）。
pub struct ToolJobRegistry {
    inner: Mutex<Inner>,
    limits: JobLimits,
}

/// 已过期 id 记录上限（TTL 清理/容量淘汰后用于把轮询区分成 `410` 而非 `404`）。
const MAX_EXPIRED_IDS: usize = 2048;

struct Inner {
    jobs: HashMap<String, JobRecord>,
    /// job 实时输出环形缓冲**侧表**（不并入 `JobRecord`，避免状态轮询克隆记录时拷贝缓冲）。
    outputs: HashMap<String, JobOutputLog>,
    /// 终态补发回调**侧表**（不并入 `JobRecord`：回调非 `Clone`，且仅 `complete` 时消费一次）。
    finished_sinks: HashMap<String, JobFinishedSink>,
    queue: VecDeque<String>,
    running: usize,
    /// 环形裁剪丢弃的元素条数（观测）。
    output_events_dropped: u64,
    /// 已被清理（TTL+宽限到期或容量淘汰）的 id：轮询区分 `JOB_EXPIRED`（410）与 `JOB_NOT_FOUND`（404）。
    /// 有界（`MAX_EXPIRED_IDS`），超限丢弃最旧（退回 `NotFound`）。
    expired: VecDeque<String>,
}

/// 登记失败原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterError {
    /// 排队已满（`max_queued`）。
    QueueFull,
    /// 条目上限（`max_entries`）且无可淘汰的终态条目。
    AtCapacity,
}

/// 取消结果（契约 §3.2：仅 `queued`/`running` 生效；已完成不可覆盖）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    /// 已转为 `cancelled`（`queued` 直接转移；`running` 仅置取消标记，状态由 worker 完成时落定）。
    Cancelled,
    /// 已是其它终态（不覆盖；HTTP 409 返回当前状态）。
    AlreadyFinished(JobStatus),
    /// 已过期被清理（HTTP 410）。
    Expired,
    NotFound,
}

/// 轮询读取结果（契约 §3.1 错误码区分）。
#[derive(Debug, Clone)]
// `Found` 携带整条 `JobRecord`；轮询频率低，体积差异可接受。
#[allow(clippy::large_enum_variant)]
pub enum GetOutcome {
    Found(JobRecord),
    /// 已过 TTL+宽限被清理（HTTP 410 `JOB_EXPIRED`）。
    Expired,
    /// 不存在 / 从未创建（HTTP 404 `JOB_NOT_FOUND`）。
    NotFound,
}

/// 输出轮询结果（`GET /tools/jobs/{id}/output`）。
#[derive(Debug, Clone)]
pub enum OutputPollOutcome {
    Found {
        /// 读取时刻状态快照（`eof` 判定基础）。
        status: JobStatus,
        /// job 归属 workspace（handler 归属校验用）。
        workspace: PathBuf,
        /// 环形缓冲增量（`eof` 已由注册表结合状态填好）。
        log_read: OutputLogRead,
        /// `true` = 任务已终态且缓冲（含终态裁剪尾部）已全部返回 → 查看者可停止。
        eof: bool,
    },
    /// 已过 TTL+宽限被清理（HTTP 410 `JOB_EXPIRED`）。
    Expired,
    /// 不存在 / 从未创建（HTTP 404 `JOB_NOT_FOUND`）。
    NotFound,
}

/// 注册表快照（观测/`/status` 用）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JobRegistryStats {
    pub total: usize,
    pub queued: usize,
    pub running: usize,
    pub terminal: usize,
    /// 全部 job 输出缓冲当前保留的文本字节合计（有界：并发 × 上限 + 终态 × 尾部）。
    pub output_retained_bytes: u64,
    /// 环形裁剪累计丢弃的元素条数。
    pub output_events_dropped: u64,
}

impl ToolJobRegistry {
    #[must_use]
    pub fn new(limits: JobLimits) -> Self {
        Self {
            inner: Mutex::new(Inner {
                jobs: HashMap::new(),
                outputs: HashMap::new(),
                finished_sinks: HashMap::new(),
                queue: VecDeque::new(),
                running: 0,
                output_events_dropped: 0,
                expired: VecDeque::new(),
            }),
            limits,
        }
    }

    #[must_use]
    pub fn limits(&self) -> JobLimits {
        self.limits
    }

    /// 登记一个 `queued` 任务并返回生成的 `tool_job_id`。
    /// 条目达上限时先淘汰最旧终态；无可淘汰则 [`RegisterError::AtCapacity`]。
    pub fn register(
        &self,
        workspace: PathBuf,
        source_turn_job_id: Option<u64>,
        spawn: super::types::JobSpawn,
        args_json: String,
        finished_sink: Option<JobFinishedSink>,
    ) -> Result<String, RegisterError> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.jobs.len() >= self.limits.max_entries && !self.evict_one_terminal_locked(&mut g) {
            return Err(RegisterError::AtCapacity);
        }
        if g.running >= self.limits.max_concurrent
            && g.queue.len() >= self.limits.max_queued
        {
            return Err(RegisterError::QueueFull);
        }
        let id = gen_tool_job_id();
        let record = JobRecord {
            id: id.clone(),
            workspace,
            source_turn_job_id,
            status: JobStatus::Queued,
            created_at: SystemTime::now(),
            finished_at: None,
            cancel_requested: false,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            spawn,
            args_json,
            workspace_changed: false,
            outcome: None,
        };
        g.jobs.insert(id.clone(), record);
        g.outputs.insert(id.clone(), JobOutputLog::default());
        if let Some(sink) = finished_sink {
            g.finished_sinks.insert(id.clone(), sink);
        }
        g.queue.push_back(id.clone());
        Ok(id)
    }

    /// 读取任务快照（轮询/worker 用；**不**做过期清理判定）。
    #[must_use]
    pub fn get(&self, id: &str) -> Option<JobRecord> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .jobs
            .get(id)
            .cloned()
    }

    /// 轮询读取（契约 §3.1）：命中记录按 TTL+宽限做**惰性**过期判定（过期即删除并记入 `expired`）；
    /// 记录不存在时按是否曾存在过区分 `Expired`（410）与 `NotFound`（404）。
    #[must_use]
    pub fn get_checked(&self, id: &str, now: SystemTime) -> GetOutcome {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(record) = g.jobs.get(id) else {
            return if g.expired.iter().any(|eid| eid == id) {
                GetOutcome::Expired
            } else {
                GetOutcome::NotFound
            };
        };
        if !record.status.is_terminal() {
            return GetOutcome::Found(record.clone());
        }
        let since_created = now.duration_since(record.created_at).unwrap_or_default();
        let since_finished = record
            .finished_at
            .and_then(|f| now.duration_since(f).ok())
            .unwrap_or_default();
        if since_created >= self.limits.ttl && since_finished >= self.limits.grace {
            g.jobs.remove(id);
            g.outputs.remove(id);
            g.finished_sinks.remove(id);
            remember_expired_locked(&mut g, id);
            return GetOutcome::Expired;
        }
        GetOutcome::Found(record.clone())
    }

    /// 追加一条实时输出（worker 的 chunk sink 调用；已按 `take_utf8_text` 组装为完整文本）。
    /// job 不存在/已清理 → `false`（worker 侧忽略即可，进程结束期间正常竞态）。
    pub fn push_output(&self, id: &str, stream: SessionStream, text: &str) -> bool {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(log) = g.outputs.get_mut(id) else {
            return false;
        };
        let dropped = log.push(stream, text, self.limits.output_buffer_bytes) as u64;
        g.output_events_dropped = g.output_events_dropped.saturating_add(dropped);
        true
    }

    /// 输出轮询（契约 §3「增量轮询」）：命中记录按 TTL+宽限做惰性过期判定；
    /// 与状态轮询共用临界区完成「增量读取 + `eof` 判定」，保证与 worker 写入/状态转移原子一致。
    #[must_use]
    pub fn poll_output(
        &self,
        id: &str,
        cursor: Option<u64>,
        now: SystemTime,
    ) -> OutputPollOutcome {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(record) = g.jobs.get(id) else {
            return if g.expired.iter().any(|eid| eid == id) {
                OutputPollOutcome::Expired
            } else {
                OutputPollOutcome::NotFound
            };
        };
        if record.status.is_terminal() {
            let since_created = now.duration_since(record.created_at).unwrap_or_default();
            let since_finished = record
                .finished_at
                .and_then(|f| now.duration_since(f).ok())
                .unwrap_or_default();
            if since_created >= self.limits.ttl && since_finished >= self.limits.grace {
                g.jobs.remove(id);
                g.outputs.remove(id);
                g.finished_sinks.remove(id);
                remember_expired_locked(&mut g, id);
                return OutputPollOutcome::Expired;
            }
        }
        let status = record.status;
        let workspace = record.workspace.clone();
        // 侧表缺失（理论不可达：register 即建）按空缓冲兜底，保证终态无输出 → eof=true。
        let (log_read, written) = match g.outputs.get(id) {
            Some(log) => {
                let read = log.read(cursor);
                (read, log.written())
            }
            None => (JobOutputLog::default().read(cursor), 0),
        };
        // eof：任务已终态且本次起点已越过全部已写元素（含终态裁剪后的尾部）。
        let eof = status.is_terminal() && log_read.next_cursor > written;
        OutputPollOutcome::Found {
            status,
            workspace,
            log_read,
            eof,
        }
    }

    /// 有空位则从 FIFO 队列取出下一个 `queued` 任务并转 `running`（worker 领取）。
    /// 返回任务快照；无空位/队列空返回 `None`。
    #[must_use]
    pub fn try_start(&self) -> Option<JobRecord> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.running >= self.limits.max_concurrent {
            return None;
        }
        let id = g.queue.pop_front()?;
        {
            let record = g.jobs.get_mut(&id)?;
            record.status = JobStatus::Running;
            record.cancel_requested = false;
        }
        g.running += 1;
        g.jobs.get(&id).cloned()
    }

    /// 取消（契约 §3.2）。`queued` 直接转 `cancelled`；`running` 置取消标记（由 worker 完成落定）。
    ///
    /// `queued` 分支不经 `complete`，故在此**自行**消费终态补发回调（契约 §5：`status` 含 `cancelled`）。
    pub fn cancel(&self, id: &str) -> CancelOutcome {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(record) = g.jobs.get_mut(id) else {
            return if g.expired.iter().any(|eid| eid == id) {
                CancelOutcome::Expired
            } else {
                CancelOutcome::NotFound
            };
        };
        match record.status {
            JobStatus::Queued => {
                let outcome = JobOutcome {
                    status: JobStatus::Cancelled,
                    exit_code: None,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    error_code: Some("cancelled".to_string()),
                    failure_category: None,
                };
                record.status = JobStatus::Cancelled;
                // 与 `complete` 同构：结果写入记录，使轮询侧 `error_code` 与 SSE 补发一致。
                record.outcome = Some(outcome.clone());
                record.finished_at = Some(SystemTime::now());
                g.queue.retain(|qid| qid != id);
                // 取出回调（一次性消费），锁外调用。
                let sink = g.finished_sinks.remove(id);
                drop(g);
                if let Some(sink) = sink {
                    sink(id, &outcome);
                }
                CancelOutcome::Cancelled
            }
            JobStatus::Running => {
                record.cancel_requested = true;
                record.cancel_flag.store(true, Ordering::SeqCst);
                CancelOutcome::Cancelled
            }
            other => CancelOutcome::AlreadyFinished(other),
        }
    }

    /// 取消本回合 `source_turn_job_id` 下尚未终态的后台工具任务（用户停止 SSE 回合时）。
    pub fn cancel_non_terminal_for_source_turn(&self, turn_job_id: u64) -> usize {
        let ids: Vec<String> = {
            let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.jobs
                .iter()
                .filter(|(_, r)| {
                    r.source_turn_job_id == Some(turn_job_id) && !r.status.is_terminal()
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        let mut n = 0;
        for id in ids {
            if matches!(
                self.cancel(&id),
                CancelOutcome::Cancelled | CancelOutcome::AlreadyFinished(JobStatus::Cancelled)
            ) {
                n += 1;
            }
        }
        n
    }

    /// 取任务的取消信号句柄（worker 启动时传入；记录不存在则 `None`）。
    #[must_use]
    pub fn cancel_flag(&self, id: &str) -> Option<Arc<AtomicBool>> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .jobs
            .get(id)
            .map(|r| Arc::clone(&r.cancel_flag))
    }

    /// worker 完成后写回结果。终态不可覆盖；`running` → 终态并递减运行计数。
    /// `workspace_changed` 由调用方按输出判定后传入。
    pub fn complete(
        &self,
        id: &str,
        outcome: JobOutcome,
        workspace_changed: bool,
    ) -> bool {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(record) = g.jobs.get_mut(id) else {
            return false;
        };
        if record.status.is_terminal() || !outcome.status.is_terminal() {
            return false;
        }
        let was_running = record.status == JobStatus::Running;
        record.status = outcome.status;
        record.outcome = Some(outcome.clone());
        record.finished_at = Some(SystemTime::now());
        record.workspace_changed = workspace_changed;
        let tail_cap = record.spawn.max_output_len;
        if was_running {
            g.running = g.running.saturating_sub(1);
        }
        // 终态裁剪：各流保留尾部 ≤ `command_max_output_len`（内存界收敛，晚到查看者仍可取最终尾部）。
        if let Some(log) = g.outputs.get_mut(id) {
            log.terminal_trim(tail_cap);
        }
        // 终态补发回调**取出**（一次性消费）；在锁外调用，避免持锁执行外部代码。
        let sink = g.finished_sinks.remove(id);
        drop(g);
        if let Some(sink) = sink {
            sink(id, &outcome);
        }
        true
    }

    /// TTL 清理：**仅终态**条目，且满足「自创建 ≥ `ttl`」与「完成后 ≥ `grace`」同时成立才删除。
    /// 返回删除条数。被清理的 id 记入 `expired`（轮询 410）。
    pub fn cleanup(&self, now: SystemTime) -> usize {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let before = g.jobs.len();
        let expired_ids: Vec<String> = g
            .jobs
            .iter()
            .filter(|(_, r)| {
                if !r.status.is_terminal() {
                    return false;
                }
                let since_created = now.duration_since(r.created_at).unwrap_or_default();
                let since_finished = r
                    .finished_at
                    .and_then(|f| now.duration_since(f).ok())
                    .unwrap_or_default();
                since_created >= self.limits.ttl && since_finished >= self.limits.grace
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in &expired_ids {
            g.jobs.remove(id);
            g.outputs.remove(id);
            g.finished_sinks.remove(id);
            remember_expired_locked(&mut g, id);
        }
        let live: std::collections::HashSet<String> = g.jobs.keys().cloned().collect();
        g.queue.retain(|id| live.contains(id));
        before - g.jobs.len()
    }

    /// 统计快照。
    #[must_use]
    pub fn stats(&self) -> JobRegistryStats {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = JobRegistryStats {
            total: g.jobs.len(),
            output_events_dropped: g.output_events_dropped,
            ..JobRegistryStats::default()
        };
        for r in g.jobs.values() {
            match r.status {
                JobStatus::Queued => s.queued += 1,
                JobStatus::Running => s.running += 1,
                _ => s.terminal += 1,
            }
        }
        for log in g.outputs.values() {
            s.output_retained_bytes = s
                .output_retained_bytes
                .saturating_add(log.retained_bytes() as u64);
        }
        s
    }

    /// 列出任务快照（最新创建在前），可选按 `workspace` 精确过滤，截断到 `limit`。
    ///
    /// - `HashMap` 遍历无序 → 显式按 `created_at` **倒序**排序，保证结果确定性；
    /// - **不做惰性过期**（过期判定归 `get_checked` / `cleanup`，避免与 `expired` 侧表语义打架）；
    /// - `workspace` 由 `enqueue_and_launch` 写入 `effective_working_dir`，与调用方同源，直接相等比较即可。
    #[must_use]
    pub fn list(&self, workspace: Option<&std::path::Path>, limit: usize) -> Vec<JobRecord> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut rows: Vec<JobRecord> = g
            .jobs
            .values()
            .filter(|r| workspace.is_none_or(|ws| r.workspace == ws))
            .cloned()
            .collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        rows.truncate(limit);
        rows
    }

    /// 淘汰最旧**终态**条目（`queued`/`running` 不可淘汰，防结果丢失）。返回是否有可淘汰项。
    fn evict_one_terminal_locked(&self, g: &mut Inner) -> bool {
        let oldest_terminal = g
            .jobs
            .iter()
            .filter(|(_, r)| r.status.is_terminal())
            .min_by_key(|(_, r)| r.created_at)
            .map(|(id, _)| id.clone());
        let Some(id) = oldest_terminal else {
            return false;
        };
        g.jobs.remove(&id);
        g.outputs.remove(&id);
        g.finished_sinks.remove(&id);
        remember_expired_locked(g, &id);
        g.queue.retain(|qid| qid != &id);
        true
    }
}

/// 把被清理的 id 记入有界 `expired` 集合（轮询 410 依据；超限丢弃最旧）。
fn remember_expired_locked(g: &mut Inner, id: &str) {
    if g.expired.len() >= MAX_EXPIRED_IDS {
        g.expired.pop_front();
    }
    if !g.expired.iter().any(|eid| eid == id) {
        g.expired.push_back(id.to_string());
    }
}

/// `tooljob_` + 32 hex 随机字节（不可枚举）。失败时回退时间戳+序列（getrandom 在主流平台几乎不失败）。
pub fn gen_tool_job_id() -> String {
    let mut buf = [0u8; 16];
    if getrandom::fill(&mut buf).is_err() {
        return fallback_id();
    }
    let hex: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    format!("tooljob_{hex}")
}

fn fallback_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    format!("tooljob_{millis:x}{:x}", SEQ.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "registry_streaming_tests.rs"]
mod streaming;

#[cfg(test)]
#[path = "registry_finished_sink_tests.rs"]
mod finished_sink;
