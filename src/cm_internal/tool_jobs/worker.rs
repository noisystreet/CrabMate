//! 后台任务执行：`tokio::spawn_blocking` + `catch_unwind`，复用 `subprocess_session`
//! （进程组 kill、并发排空管道、墙钟、截断缓冲）。
//!
//! - 命令由调用方（`run_command` 的 `async=true` 路径）完成白名单 / 路径 / 审批校验后传入。
//! - panic 兜底：`catch_unwind` + `JoinError` → `failed(internal)`，**不**卡 `running` 直至 TTL。
//!   （`wait_child_session` 内部错误路径均已处理，正常不 panic；极端 panic 时子进程随会话对象
//!   drop 释放，进程组清理不承诺，见 ADR「崩溃不恢复」。）
//! - 调度：`enqueue_and_launch` 登记后立即 `drain_queued`；任一任务完成（`launch_job` 回调）
//!   空出并发位后再次 `drain_queued`，FIFO 领取队列中的下一个。

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::cm_tools::subprocess_session::{
    SessionChunkSink, SessionStopKind, SessionStream, SubprocessWaitCtl,
    prepare_piped_process_group, take_utf8_text, wait_child_session,
};

use super::registry::{OutputPollOutcome, RegisterError, ToolJobRegistry};
use super::types::{JobOutcome, JobStatus};

pub use super::types::JobSpawn;

/// 实时输出文本回调（已由 [`chunk_sink_from_output`] 完成 UTF-8 组装；每调用一条完整文本）。
pub type JobOutputSink = Arc<dyn Fn(SessionStream, &str) + Send + Sync>;

/// 把文本回调包装成 `subprocess_session` 的字节级 `chunk_sink`：
/// 跨块保持 UTF-8 不完整序列（`pending` 缓冲），流结束（空字节标记）时 flush 尾部非法字节。
fn chunk_sink_from_output(output: JobOutputSink) -> SessionChunkSink {
    let stdout_pending = Arc::new(Mutex::new(Vec::<u8>::new()));
    let stderr_pending = Arc::new(Mutex::new(Vec::<u8>::new()));
    Arc::new(move |stream, bytes| {
        let (pending, out) = match stream {
            SessionStream::Stdout => (Arc::clone(&stdout_pending), Arc::clone(&output)),
            SessionStream::Stderr => (Arc::clone(&stderr_pending), Arc::clone(&output)),
        };
        let mut buf = pending.lock().unwrap_or_else(|e| e.into_inner());
        let text = take_utf8_text(&mut buf, bytes, bytes.is_empty());
        drop(buf);
        if !text.is_empty() {
            out(stream, &text);
        }
        true
    })
}

/// 同步执行（阻塞调用线程）。超时/取消/正常退出按会话结果映射为 [`JobOutcome`]。
/// `cancel` 为注册表持有的取消信号（与 `registry.cancel` 共享同一 `Arc`）。
/// `output` 为实时输出回调（`Some` 时 `uncapped_live=true`，输出不被 `max_output_len` 截断）。
pub fn run_job_blocking(
    spawn: JobSpawn,
    cancel: Arc<AtomicBool>,
    output: Option<JobOutputSink>,
) -> JobOutcome {
    let mut cmd = spawn.to_command();
    prepare_piped_process_group(&mut cmd);
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let mut out = JobOutcome::failed("spawn_failed");
            out.stderr = format!("无法启动命令：{e}").into_bytes();
            return out;
        }
    };
    let chunk_sink = output.as_ref().map(|sink| chunk_sink_from_output(Arc::clone(sink)));
    let ctl = SubprocessWaitCtl {
        wall: Some(spawn.wall),
        cancel: Some(cancel),
        extra_stop: None,
        chunk_sink,
        uncapped_live: output.is_some(),
    };
    let session = match wait_child_session(child, &ctl, spawn.max_output_len) {
        Ok(s) => s,
        Err(e) => {
            let mut out = JobOutcome::failed("wait_failed");
            out.stderr = format!("等待子进程失败：{e}").into_bytes();
            return out;
        }
    };
    let (status, error_code) = match session.kind {
        SessionStopKind::Exited => {
            let ok = session.status.is_some_and(|s| s.success());
            (
                if ok {
                    JobStatus::Succeeded
                } else {
                    JobStatus::Failed
                },
                None,
            )
        }
        SessionStopKind::Timeout => (JobStatus::TimedOut, Some("timeout".to_string())),
        SessionStopKind::Cancelled => (JobStatus::Cancelled, Some("cancelled".to_string())),
    };
    JobOutcome {
        status,
        exit_code: session.status.and_then(|s| s.code()),
        stdout: session.stdout,
        stderr: session.stderr,
        error_code,
        failure_category: None,
    }
}

/// 后台启动：`spawn_blocking` 内执行 + `catch_unwind`（panic → `failed(internal)`），
/// 完成后把结果（含调用方判定的 `workspace_changed`）写回注册表，并尝试领取队列中的下一个任务。
///
/// 返回 `JoinHandle` 供观测/测试等待。
pub fn launch_job(
    registry: Arc<ToolJobRegistry>,
    job_id: String,
    spawn: JobSpawn,
    workspace_changed: Arc<dyn Fn(&JobOutcome) -> bool + Send + Sync>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let cancel = registry
            .cancel_flag(&job_id)
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        let output: JobOutputSink = {
            let reg = Arc::clone(&registry);
            let id = job_id.clone();
            Arc::new(move |stream, text| {
                let _ = reg.push_output(&id, stream, text);
            })
        };
        let outcome = tokio::task::spawn_blocking(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_job_blocking(spawn, cancel, Some(output))
            }))
            .unwrap_or_else(|_| JobOutcome::failed("internal"))
        })
        .await
        .unwrap_or_else(|_| JobOutcome::failed("internal"));
        let wc = workspace_changed(&outcome);
        registry.complete(&job_id, outcome, wc);
        // 空出并发位后按 FIFO 领取下一个排队任务。
        drain_queued(Arc::clone(&registry));
    })
}

/// `run_command` 后台任务：终态 `workspace_changed` 判定（与同步路径
/// `tools::is_compile_command_success` 等价：编译命令 + 退出码 0）。
#[must_use]
pub fn job_outcome_workspace_changed(args_json: &str, outcome: &JobOutcome) -> bool {
    if outcome.status != JobStatus::Succeeded {
        return false;
    }
    let cmd = match serde_json::from_str::<serde_json::Value>(args_json).ok() {
        Some(v) => v
            .get("command")
            .and_then(|c| c.as_str())
            .map(|s| s.trim().to_lowercase())
            .unwrap_or_default(),
        None => String::new(),
    };
    matches!(
        cmd.as_str(),
        "gcc" | "g++" | "clang" | "clang++" | "make" | "cmake" | "ninja"
    )
}

/// 尽最大努力领取队列任务（FIFO）并启动；无空位/队列空即返回。
/// 完成后由 [`launch_job`] 再次调用，形成「完成 → 领下一个」的调度环。
pub fn drain_queued(registry: Arc<ToolJobRegistry>) {
    while let Some(rec) = registry.try_start() {
        let args_json = rec.args_json.clone();
        let wc: Arc<dyn Fn(&JobOutcome) -> bool + Send + Sync> =
            Arc::new(move |o| job_outcome_workspace_changed(&args_json, o));
        launch_job(Arc::clone(&registry), rec.id.clone(), rec.spawn.clone(), wc);
    }
}

/// 登记后台任务并立即尝试启动（并发满则入队，完成后由调度环领取）。
/// 返回 `tool_job_id`。
pub fn enqueue_and_launch(
    registry: Arc<ToolJobRegistry>,
    workspace: PathBuf,
    source_turn_job_id: Option<u64>,
    spawn: JobSpawn,
    args_json: String,
) -> Result<String, RegisterError> {
    let id = registry.register(workspace, source_turn_job_id, spawn, args_json)?;
    drain_queued(Arc::clone(&registry));
    Ok(id)
}

/// 周期性 TTL 清理任务（进程内定时器）。
pub fn spawn_cleanup_task(
    registry: Arc<ToolJobRegistry>,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            let removed = registry.cleanup(std::time::SystemTime::now());
            if removed > 0 {
                log::info!(
                    target: "crabmate",
                    "tool job cleanup removed={} remaining={}",
                    removed,
                    registry.stats().total
                );
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn outcome_from(cmd: &mut Command, wall: Duration) -> JobOutcome {
        run_job_blocking(
            JobSpawn::from_command(cmd, wall, 4096),
            Arc::new(AtomicBool::new(false)),
            None,
        )
    }

    fn test_limits() -> crate::cm_internal::tool_jobs::types::JobLimits {
        crate::cm_internal::tool_jobs::types::JobLimits {
            max_concurrent: 4,
            max_queued: 32,
            ttl: Duration::from_secs(3600),
            grace: Duration::from_secs(60),
            max_entries: 128,
            output_buffer_bytes: 262_144,
        }
    }

    #[test]
    fn run_job_success_captures_stdout() {
        let mut cmd = Command::new("echo");
        cmd.arg("job-ok");
        let o = outcome_from(&mut cmd, Duration::from_secs(5));
        assert_eq!(o.status, JobStatus::Succeeded);
        assert_eq!(o.exit_code, Some(0));
        assert!(String::from_utf8_lossy(&o.stdout).contains("job-ok"));
    }

    #[test]
    fn run_job_nonzero_exit_is_failed() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "exit 3"]);
        let o = outcome_from(&mut cmd, Duration::from_secs(5));
        assert_eq!(o.status, JobStatus::Failed);
        assert_eq!(o.exit_code, Some(3));
        assert_eq!(o.error_code, None);
    }

    #[cfg(unix)]
    #[test]
    fn run_job_timeout_kills_process_group() {
        let marker = format!("cm_job_timeout_{}", std::process::id());
        let mut cmd = Command::new("bash");
        cmd.args(["-c", &format!("sleep 60 # {marker}")]);
        let o = outcome_from(&mut cmd, Duration::from_secs(1));
        assert_eq!(o.status, JobStatus::TimedOut);
        assert_eq!(o.error_code.as_deref(), Some("timeout"));
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(
            !crate::cm_tools::subprocess_session::proc_cmdline_contains(&marker),
            "孙进程 sleep 仍在运行（进程组未杀干净）"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_job_cancel_stops_sleep() {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut cmd = Command::new("sleep");
        cmd.arg("60");
        let cancel_th = Arc::clone(&cancel);
        let handle = std::thread::spawn(move || {
            run_job_blocking(
                JobSpawn::from_command(&mut cmd, Duration::from_secs(30), 1024),
                cancel_th,
                None,
            )
        });
        std::thread::sleep(std::time::Duration::from_millis(150));
        cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        let o = handle.join().expect("join");
        assert_eq!(o.status, JobStatus::Cancelled);
        assert_eq!(o.error_code.as_deref(), Some("cancelled"));
    }

    #[tokio::test]
    async fn launch_job_completes_registry_and_writes_workspace_changed() {
        let reg = Arc::new(ToolJobRegistry::new(test_limits()));
        let id = enqueue_and_launch(
            Arc::clone(&reg),
            std::path::PathBuf::from("/ws"),
            None,
            JobSpawn {
                program: "echo".to_string(),
                args: vec!["async-ok".to_string()],
                cwd: std::path::PathBuf::from("/"),
                extra_env: Vec::new(),
                wall: Duration::from_secs(5),
                max_output_len: 4096,
            },
            r#"{"command":"echo"}"#.to_string(),
        )
        .expect("enqueue");
        // enqueue_and_launch 已启动；等 worker 完成。
        let rec = loop {
            let rec = reg.get(&id).expect("record");
            if rec.status.is_terminal() {
                break rec;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert_eq!(rec.status, JobStatus::Succeeded);
        assert!(String::from_utf8_lossy(
            &rec.outcome.as_ref().expect("out").stdout
        )
        .contains("async-ok"));
        // 编译命令判定：echo 非编译命令 → false。
        assert!(!rec.workspace_changed, "非编译命令不得标记 workspace_changed");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn launch_job_cancel_via_registry_stops_process() {
        let reg = Arc::new(ToolJobRegistry::new(test_limits()));
        let id = enqueue_and_launch(
            Arc::clone(&reg),
            std::path::PathBuf::from("/ws"),
            None,
            JobSpawn {
                program: "sleep".to_string(),
                args: vec!["60".to_string()],
                cwd: std::path::PathBuf::from("/"),
                extra_env: Vec::new(),
                wall: Duration::from_secs(30),
                max_output_len: 1024,
            },
            r#"{"command":"sleep"}"#.to_string(),
        )
        .expect("enqueue");
        // 等 worker 真正进入等待循环后经注册表取消（验证 cancel_flag 接线）。
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(reg.cancel(&id), crate::cm_internal::tool_jobs::registry::CancelOutcome::Cancelled);
        let rec = loop {
            let rec = reg.get(&id).expect("record");
            if rec.status.is_terminal() {
                break rec;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert_eq!(rec.status, JobStatus::Cancelled);
        assert_eq!(rec.outcome.as_ref().expect("out").error_code.as_deref(), Some("cancelled"));
    }

    #[tokio::test]
    async fn enqueue_respects_concurrency_and_drains_queued() {
        let reg = Arc::new(ToolJobRegistry::new(
            crate::cm_internal::tool_jobs::types::JobLimits {
                max_concurrent: 1,
                max_queued: 8,
                ttl: Duration::from_secs(3600),
                grace: Duration::from_secs(60),
                max_entries: 128,
                output_buffer_bytes: 262_144,
            },
        ));
        let spawn = |program: &str, secs: u64| JobSpawn {
            program: program.to_string(),
            args: vec![secs.to_string()],
            cwd: std::path::PathBuf::from("/"),
            extra_env: Vec::new(),
            wall: Duration::from_secs(20),
            max_output_len: 1024,
        };
        let args = |cmd: &str| format!(r#"{{"command":"{cmd}"}}"#);
        // 第 1 个占用唯一并发位；后两个入队。
        let id1 = enqueue_and_launch(
            Arc::clone(&reg),
            PathBuf::from("/ws"),
            None,
            spawn("sleep", 0),
            args("sleep"),
        )
        .expect("enqueue1");
        let id2 = enqueue_and_launch(
            Arc::clone(&reg),
            PathBuf::from("/ws"),
            None,
            spawn("sleep", 0),
            args("sleep"),
        )
        .expect("enqueue2");
        let id3 = enqueue_and_launch(
            Arc::clone(&reg),
            PathBuf::from("/ws"),
            None,
            spawn("sleep", 0),
            args("sleep"),
        )
        .expect("enqueue3");
        async fn wait_terminal(reg: &ToolJobRegistry, id: &str) -> JobStatus {
            loop {
                let rec = reg.get(id).expect("record");
                if rec.status.is_terminal() {
                    return rec.status;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
        // FIFO：id1 先完成，之后 id2 → id3 依次被调度环领取。
        assert_eq!(wait_terminal(&reg, &id1).await, JobStatus::Succeeded);
        assert_eq!(wait_terminal(&reg, &id2).await, JobStatus::Succeeded);
        assert_eq!(wait_terminal(&reg, &id3).await, JobStatus::Succeeded);
        assert_eq!(reg.stats().running, 0);
        assert_eq!(reg.stats().queued, 0);
    }

    #[test]
    fn workspace_changed_only_for_compile_commands() {
        let ok = JobOutcome {
            status: JobStatus::Succeeded,
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            error_code: None,
            failure_category: None,
        };
        assert!(job_outcome_workspace_changed(
            r#"{"command":"make","args":["-j4"]}"#,
            &ok
        ));
        assert!(!job_outcome_workspace_changed(
            r#"{"command":"ls"}"#,
            &ok
        ));
        let failed = JobOutcome {
            status: JobStatus::Failed,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: Vec::new(),
            error_code: None,
            failure_category: None,
        };
        assert!(!job_outcome_workspace_changed(
            r#"{"command":"make"}"#,
            &failed
        ));
    }

    /// 后台任务输出全程可达环形缓冲（`uncapped_live`）：即使超过 `max_output_len`，
    /// `poll_output` 也能增量取到全部输出并以 `eof=true` 收尾；终态快照仍按 cap 前缀截断。
    ///
    /// 输出先全部落盘、再 `sleep 0.4s` 保持 running：给轮询留出"运行期抓全量"的窗口，
    /// 避免 `complete()` 终态裁剪（各流尾部 ≤ max_output_len）先行丢早段事件。
    #[cfg(unix)]
    #[tokio::test]
    async fn enqueue_job_output_streams_beyond_capture_cap_and_ends_eof() {
        let reg = Arc::new(ToolJobRegistry::new(test_limits()));
        let id = enqueue_and_launch(
            Arc::clone(&reg),
            PathBuf::from("/ws"),
            None,
            JobSpawn {
                program: "bash".to_string(),
                args: vec![
                    "-c".to_string(),
                    "awk 'BEGIN{for(i=0;i<6000;i++)printf \"a\"; print \" tail-line\"}' ; sleep 0.4"
                        .to_string(),
                ],
                cwd: PathBuf::from("/"),
                extra_env: Vec::new(),
                wall: Duration::from_secs(20),
                max_output_len: 2048,
            },
            r#"{"command":"bash"}"#.to_string(),
        )
        .expect("enqueue");
        let mut cursor: Option<u64> = None;
        let mut joined = String::new();
        let mut saw_eof = false;
        let mut saw_terminal_status = false;
        for _ in 0..4000 {
            match reg.poll_output(&id, cursor, std::time::SystemTime::now()) {
                OutputPollOutcome::Found {
                    status,
                    log_read,
                    eof,
                    ..
                } => {
                    saw_terminal_status = status.is_terminal();
                    for it in &log_read.items {
                        joined.push_str(&it.text);
                    }
                    cursor = Some(log_read.next_cursor);
                    saw_eof = eof;
                    if eof {
                        break;
                    }
                }
                _ => panic!("job 输出缓冲缺失（理论不可达）"),
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(joined.contains("tail-line"), "joined: {joined:?}");
        assert!(
            joined.len() >= 6000,
            "全量输出应可达环形缓冲（> max_output_len），len={}",
            joined.len()
        );
        assert!(saw_terminal_status, "eof 前应看到终态");
        assert!(saw_eof, "轮询应以 eof=true 收尾");
        let rec = reg.get(&id).expect("rec");
        assert_eq!(rec.status, JobStatus::Succeeded);
        let kept = &rec.outcome.as_ref().expect("out").stdout;
        assert!(
            kept.len() <= 2048,
            "终态快照仍按 max_output_len 前缀截断，len={}",
            kept.len()
        );
        // 终态裁剪：poll 之后缓冲仍保留最终尾部可读。
        let tail = reg.poll_output(&id, None, std::time::SystemTime::now());
        let OutputPollOutcome::Found { log_read, .. } = tail else {
            panic!("终态后应仍可取输出");
        };
        let tail_text: String = log_read.items.iter().map(|e| e.text.as_str()).collect();
        assert!(tail_text.contains("tail-line"), "终态尾部应可读: {tail_text:?}");
    }
}
