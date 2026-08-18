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

use super::types::{JobLimits, JobOutcome, JobRecord, JobSpawn, JobStatus};

/// 进程内注册表（所有操作持锁，单临界区保证状态转移原子性）。
pub struct ToolJobRegistry {
    inner: Mutex<Inner>,
    limits: JobLimits,
}

/// 已过期 id 记录上限（TTL 清理/容量淘汰后用于把轮询区分成 `410` 而非 `404`）。
const MAX_EXPIRED_IDS: usize = 2048;

struct Inner {
    jobs: HashMap<String, JobRecord>,
    queue: VecDeque<String>,
    running: usize,
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

/// 注册表快照（观测/`/status` 用）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JobRegistryStats {
    pub total: usize,
    pub queued: usize,
    pub running: usize,
    pub terminal: usize,
}

impl ToolJobRegistry {
    #[must_use]
    pub fn new(limits: JobLimits) -> Self {
        Self {
            inner: Mutex::new(Inner {
                jobs: HashMap::new(),
                queue: VecDeque::new(),
                running: 0,
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
            remember_expired_locked(&mut g, id);
            return GetOutcome::Expired;
        }
        GetOutcome::Found(record.clone())
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
                record.status = JobStatus::Cancelled;
                record.finished_at = Some(SystemTime::now());
                g.queue.retain(|qid| qid != id);
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
        record.outcome = Some(outcome);
        record.finished_at = Some(SystemTime::now());
        record.workspace_changed = workspace_changed;
        if was_running {
            g.running = g.running.saturating_sub(1);
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
            ..JobRegistryStats::default()
        };
        for r in g.jobs.values() {
            match r.status {
                JobStatus::Queued => s.queued += 1,
                JobStatus::Running => s.running += 1,
                _ => s.terminal += 1,
            }
        }
        s
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
mod tests {
    use super::*;
    use std::time::Duration;

    fn limits() -> JobLimits {
        JobLimits {
            max_concurrent: 2,
            max_queued: 2,
            ttl: Duration::from_secs(3600),
            grace: Duration::from_secs(60),
            max_entries: 4,
        }
    }

    fn job_id(r: &JobRecord) -> String {
        r.id.clone()
    }

    fn spawn_default() -> JobSpawn {
        JobSpawn {
            program: "true".to_string(),
            args: Vec::new(),
            cwd: PathBuf::from("/"),
            extra_env: Vec::new(),
            wall: Duration::from_secs(10),
            max_output_len: 1024,
        }
    }

    fn register_default(reg: &ToolJobRegistry) -> String {
        reg.register(
            PathBuf::from("/w"),
            None,
            spawn_default(),
            r#"{"command":"true"}"#.to_string(),
        )
        .expect("register")
    }

    #[test]
    fn register_queues_and_get_returns_record() {
        let reg = ToolJobRegistry::new(limits());
        let id = reg
            .register(
                PathBuf::from("/ws"),
                Some(7),
                spawn_default(),
                r#"{"command":"true"}"#.to_string(),
            )
            .expect("register");
        assert!(id.starts_with("tooljob_"), "id: {id}");
        assert_eq!(id.len(), "tooljob_".len() + 32);
        let rec = reg.get(&id).expect("record");
        assert_eq!(rec.status, JobStatus::Queued);
        assert_eq!(rec.workspace, PathBuf::from("/ws"));
        assert_eq!(rec.source_turn_job_id, Some(7));
    }

    #[test]
    fn try_start_fifo_and_respects_max_concurrent() {
        let reg = ToolJobRegistry::new(limits());
        let a = register_default(&reg); // a
        let b = register_default(&reg); // b
        let c = register_default(&reg); // 第 3 个：进入队列
        let r1 = reg.try_start().expect("r1");
        let r2 = reg.try_start().expect("r2");
        assert_eq!(job_id(&r1), a);
        assert_eq!(job_id(&r2), b);
        assert!(reg.try_start().is_none(), "并发满，不得再领取");
        assert_eq!(reg.get(&c).expect("c").status, JobStatus::Queued);
        assert_eq!(reg.stats().running, 2);
        assert_eq!(reg.stats().queued, 1);
    }

    #[test]
    fn register_rejects_when_queue_full() {
        let reg = ToolJobRegistry::new(JobLimits {
            max_entries: 100,
            ..limits()
        }); // concurrent=2, queued=2；entries 上限放大以免先触发 AtCapacity
        register_default(&reg); // a
        register_default(&reg); // b
        reg.try_start().expect("r1");
        reg.try_start().expect("r2"); // 并发已满
        register_default(&reg); // c
        register_default(&reg); // 队列已满
        assert_eq!(
            reg.register(
                PathBuf::from("/w"),
                None,
                spawn_default(),
                r#"{"command":"true"}"#.to_string()
            ),
            Err(RegisterError::QueueFull)
        );
    }

    #[test]
    fn cancel_queued_moves_terminal_and_removes_from_queue() {
        let reg = ToolJobRegistry::new(limits());
        let a = register_default(&reg); // a
        let b = register_default(&reg); // b
        assert_eq!(reg.cancel(&a), CancelOutcome::Cancelled);
        assert_eq!(reg.get(&a).expect("a").status, JobStatus::Cancelled);
        assert!(reg.get(&a).expect("a").finished_at.is_some());
        // 队列不再含 a：领取到的应为 b
        let r = reg.try_start().expect("start");
        assert_eq!(job_id(&r), b);
    }

    #[test]
    fn cancel_running_sets_flag_then_complete_transitions() {
        let reg = ToolJobRegistry::new(limits());
        let id = register_default(&reg); // id
        reg.try_start().expect("start");
        let flag = reg.cancel_flag(&id).expect("cancel flag");
        assert_eq!(reg.cancel(&id), CancelOutcome::Cancelled);
        assert!(reg.get(&id).expect("rec").cancel_requested);
        assert!(flag.load(Ordering::SeqCst), "worker 取消信号应被置位");
        assert_eq!(reg.get(&id).expect("rec").status, JobStatus::Running);
        let outcome = JobOutcome {
            status: JobStatus::Cancelled,
            exit_code: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            error_code: Some("cancelled".into()),
            failure_category: None,
        };
        assert!(reg.complete(&id, outcome, false));
        let rec = reg.get(&id).expect("rec");
        assert_eq!(rec.status, JobStatus::Cancelled);
        assert_eq!(rec.outcome.as_ref().expect("out").error_code.as_deref(), Some("cancelled"));
        assert_eq!(reg.stats().running, 0);
    }

    #[test]
    fn complete_rejects_non_terminal_outcome() {
        let reg = ToolJobRegistry::new(limits());
        let id = register_default(&reg); // id
        reg.try_start().expect("start");
        let running = JobOutcome {
            status: JobStatus::Running,
            exit_code: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            error_code: None,
            failure_category: None,
        };
        assert!(!reg.complete(&id, running, false), "非终态 outcome 不得写入");
        assert_eq!(reg.get(&id).expect("rec").status, JobStatus::Running);
        assert_eq!(reg.stats().running, 1);
    }

    #[test]
    fn complete_rejects_terminal_overwrite_and_unknown() {
        let reg = ToolJobRegistry::new(limits());
        let id = register_default(&reg); // id
        let ok = JobOutcome {
            status: JobStatus::Succeeded,
            exit_code: Some(0),
            stdout: b"out".to_vec(),
            stderr: Vec::new(),
            error_code: None,
            failure_category: None,
        };
        assert!(reg.complete(&id, ok.clone(), true));
        assert!(!reg.complete(&id, ok.clone(), false), "终态不可覆盖");
        assert!(!reg.complete("tooljob_missing", ok.clone(), false));
        assert!(reg.get(&id).expect("rec").workspace_changed);
    }

    #[test]
    fn cleanup_removes_only_terminal_past_ttl_and_grace() {
        let reg = ToolJobRegistry::new(JobLimits {
            ttl: Duration::from_secs(100),
            grace: Duration::from_secs(10),
            ..limits()
        });
        let id = register_default(&reg); // id
        let ok = JobOutcome {
            status: JobStatus::Succeeded,
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            error_code: None,
            failure_category: None,
        };
        reg.complete(&id, ok, false);
        let now = SystemTime::now();
        // 完成但未过 grace：保留
        assert_eq!(reg.cleanup(now), 0);
        // 完成且过 grace（由 `finished_at` 起算），但自创建不足 ttl：仍保留（ttl 自创建算）
        let finished = reg.get(&id).expect("rec").finished_at.expect("finished");
        let later = finished + Duration::from_secs(20);
        assert_eq!(reg.cleanup(later), 0, "ttl 未到不可删");
        // 同时过 ttl 与 grace：删除
        let far = reg.get(&id).expect("rec").created_at + Duration::from_secs(200);
        assert_eq!(reg.cleanup(far), 1);
        assert!(reg.get(&id).is_none());
    }

    #[test]
    fn cleanup_never_removes_running() {
        let reg = ToolJobRegistry::new(limits());
        let id = register_default(&reg); // id
        reg.try_start().expect("start");
        let far = SystemTime::now() + Duration::from_secs(10_000);
        assert_eq!(reg.cleanup(far), 0);
        assert_eq!(reg.get(&id).expect("rec").status, JobStatus::Running);
    }

    #[test]
    fn eviction_only_terminal_and_lowest_created() {
        let reg = ToolJobRegistry::new(JobLimits {
            max_entries: 2,
            ..limits()
        });
        let a = register_default(&reg); // a
        let b = register_default(&reg); // b
        // 无终态可淘汰：AtCapacity
        assert_eq!(
            reg.register(
                PathBuf::from("/w"),
                None,
                spawn_default(),
                r#"{"command":"true"}"#.to_string()
            ),
            Err(RegisterError::AtCapacity)
        );
        // 完成 a（终态）后注册 c → 淘汰 a
        let ok = JobOutcome {
            status: JobStatus::Succeeded,
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            error_code: None,
            failure_category: None,
        };
        reg.complete(&a, ok, false);
        let c = register_default(&reg); // c
        assert!(reg.get(&a).is_none(), "最旧终态应被淘汰");
        assert!(reg.get(&b).is_some());
        assert!(reg.get(&c).is_some());
    }

    #[test]
    fn gen_id_is_opaque_hex() {
        let a = gen_tool_job_id();
        let b = gen_tool_job_id();
        assert_ne!(a, b);
        assert!(a.starts_with("tooljob_"));
        let hex = a.trim_start_matches("tooljob_");
        assert_eq!(hex.len(), 32);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn get_checked_found_not_found_and_lazy_expiry() {
        let reg = ToolJobRegistry::new(JobLimits {
            ttl: Duration::from_secs(100),
            grace: Duration::from_secs(10),
            ..limits()
        });
        let id = register_default(&reg); // id
        // 非终态（queued）：直接 Found，不过期。
        assert!(matches!(
            reg.get_checked(&id, SystemTime::now() + Duration::from_secs(10_000)),
            GetOutcome::Found(_)
        ));
        // 从未创建：NotFound。
        assert!(matches!(
            reg.get_checked("tooljob_unknown", SystemTime::now()),
            GetOutcome::NotFound
        ));
        // 完成后已过 grace 且自创建过 ttl：惰性判定 Expired 并删除。
        let ok = JobOutcome {
            status: JobStatus::Succeeded,
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            error_code: None,
            failure_category: None,
        };
        assert!(reg.complete(&id, ok, false));
        let rec = reg.get(&id).expect("rec");
        let expired_at = rec.created_at + Duration::from_secs(200);
        assert!(matches!(reg.get_checked(&id, expired_at), GetOutcome::Expired));
        assert!(reg.get(&id).is_none());
        // 删除后仍能识别为 Expired（而非 NotFound）。
        assert!(matches!(reg.get_checked(&id, expired_at), GetOutcome::Expired));
    }

    #[test]
    fn cleanup_remembers_expired_then_cancel_reports_expired() {
        let reg = ToolJobRegistry::new(JobLimits {
            ttl: Duration::from_secs(100),
            grace: Duration::from_secs(10),
            ..limits()
        });
        let id = register_default(&reg); // id
        let ok = JobOutcome {
            status: JobStatus::Succeeded,
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            error_code: None,
            failure_category: None,
        };
        reg.complete(&id, ok, false);
        let far = SystemTime::now() + Duration::from_secs(10_000);
        assert_eq!(reg.cleanup(far), 1);
        assert!(reg.get(&id).is_none());
        assert_eq!(reg.cancel(&id), CancelOutcome::Expired);
        assert_eq!(reg.cancel("tooljob_never"), CancelOutcome::NotFound);
        // 被淘汰的终态同样记入 expired。
        let reg2 = ToolJobRegistry::new(JobLimits {
            max_entries: 2,
            ..limits()
        });
        let a = register_default(&reg2); // a
        let b = register_default(&reg2); // b（占用全部条目，无终态可淘汰）
        let ok2 = JobOutcome {
            status: JobStatus::Succeeded,
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            error_code: None,
            failure_category: None,
        };
        reg2.complete(&a, ok2, false);
        register_default(&reg2); // 触发淘汰终态 a
        assert!(reg2.get(&b).is_some(), "非终态 b 不可被淘汰");
        assert_eq!(reg2.cancel(&a), CancelOutcome::Expired);
    }

    #[test]
    fn get_checked_does_not_expire_running_or_queued() {
        let reg = ToolJobRegistry::new(JobLimits {
            ttl: Duration::from_secs(1),
            grace: Duration::from_secs(1),
            ..limits()
        });
        let queued = register_default(&reg); // queued
        let running = register_default(&reg); // running
        reg.try_start().expect("start");
        let far = SystemTime::now() + Duration::from_secs(3600);
        assert!(matches!(reg.get_checked(&queued, far), GetOutcome::Found(_)));
        assert!(matches!(reg.get_checked(&running, far), GetOutcome::Found(_)));
        assert_eq!(reg.stats().total, 2, "非终态不得被惰性清理");
    }
}
