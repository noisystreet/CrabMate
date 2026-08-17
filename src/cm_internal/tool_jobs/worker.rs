//! 后台任务执行：`tokio::spawn_blocking` + `catch_unwind`，复用 `subprocess_session`
//! （进程组 kill、并发排空管道、墙钟、截断缓冲）。
//!
//! - 命令由调用方（`run_command` 的 `async=true` 路径）完成白名单 / 路径 / 审批校验后传入。
//! - panic 兜底：`catch_unwind` + `JoinError` → `failed(internal)`，**不**卡 `running` 直至 TTL。
//!   （`wait_child_session` 内部错误路径均已处理，正常不 panic；极端 panic 时子进程随会话对象
//!   drop 释放，进程组清理不承诺，见 ADR「崩溃不恢复」。）

use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use crate::cm_tools::subprocess_session::{
    SessionStopKind, SubprocessWaitCtl, prepare_piped_process_group, wait_child_session,
};

use super::registry::ToolJobRegistry;
use super::types::{JobOutcome, JobStatus};

/// 一次后台执行的参数（命令已通过白名单/路径/审批）。
///
/// 取消信号不在此处：由注册表持有一份（`JobRecord.cancel_flag`），`launch_job` 启动时
/// 从注册表取同一次 `Arc<AtomicBool>` 传入，`registry.cancel()` 才能命中同一信号。
pub struct JobSpawn {
    pub command: Command,
    /// 墙钟；超时对进程组 SIGTERM→SIGKILL。
    pub wall: Duration,
    /// 输出截断上限（复用 `command_max_output_len`）。
    pub max_output_len: usize,
}

/// 同步执行（阻塞调用线程）。超时/取消/正常退出按会话结果映射为 [`JobOutcome`]。
/// `cancel` 为注册表持有的取消信号（与 `registry.cancel` 共享同一 `Arc`）。
pub fn run_job_blocking(spawn: JobSpawn, cancel: Arc<AtomicBool>) -> JobOutcome {
    let mut cmd = spawn.command;
    prepare_piped_process_group(&mut cmd);
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let mut out = JobOutcome::failed("spawn_failed");
            out.stderr = format!("无法启动命令：{e}").into_bytes();
            return out;
        }
    };
    let ctl = SubprocessWaitCtl {
        wall: Some(spawn.wall),
        cancel: Some(cancel),
        extra_stop: None,
        chunk_sink: None,
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
/// 完成后把结果（含调用方判定的 `workspace_changed`）写回注册表。
///
/// 返回 `JoinHandle` 供观测/测试等待；调度循环见后续切片（`run_command` 集成处按
/// `max_concurrent` 领取 `try_start`）。
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
        let outcome = tokio::task::spawn_blocking(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_job_blocking(spawn, cancel)
            }))
            .unwrap_or_else(|_| JobOutcome::failed("internal"))
        })
        .await
        .unwrap_or_else(|_| JobOutcome::failed("internal"));
        let wc = workspace_changed(&outcome);
        registry.complete(&job_id, outcome, wc);
    })
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

    fn outcome_from(cmd: Command, wall: Duration) -> JobOutcome {
        run_job_blocking(
            JobSpawn {
                command: cmd,
                wall,
                max_output_len: 4096,
            },
            Arc::new(AtomicBool::new(false)),
        )
    }

    #[test]
    fn run_job_success_captures_stdout() {
        let mut cmd = Command::new("echo");
        cmd.arg("job-ok");
        let o = outcome_from(cmd, Duration::from_secs(5));
        assert_eq!(o.status, JobStatus::Succeeded);
        assert_eq!(o.exit_code, Some(0));
        assert!(String::from_utf8_lossy(&o.stdout).contains("job-ok"));
    }

    #[test]
    fn run_job_nonzero_exit_is_failed() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "exit 3"]);
        let o = outcome_from(cmd, Duration::from_secs(5));
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
        let o = outcome_from(cmd, Duration::from_secs(1));
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
                JobSpawn {
                    command: cmd,
                    wall: Duration::from_secs(30),
                    max_output_len: 1024,
                },
                cancel_th,
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
        let reg = Arc::new(ToolJobRegistry::new(
            crate::cm_internal::tool_jobs::types::JobLimits {
                max_concurrent: 4,
                max_queued: 32,
                ttl: Duration::from_secs(3600),
                grace: Duration::from_secs(60),
                max_entries: 128,
            },
        ));
        let id = reg
            .register(std::path::PathBuf::from("/ws"), None)
            .expect("register");
        reg.try_start().expect("start");

        let mut cmd = Command::new("echo");
        cmd.arg("async-ok");
        let spawn = JobSpawn {
            command: cmd,
            wall: Duration::from_secs(5),
            max_output_len: 4096,
        };
        let wc: Arc<dyn Fn(&JobOutcome) -> bool + Send + Sync> = Arc::new(|_| true);
        launch_job(Arc::clone(&reg), id.clone(), spawn, wc)
            .await
            .expect("join");
        let rec = reg.get(&id).expect("record");
        assert_eq!(rec.status, JobStatus::Succeeded);
        assert!(String::from_utf8_lossy(
            &rec.outcome.as_ref().expect("out").stdout
        )
        .contains("async-ok"));
        assert!(rec.workspace_changed, "调用方判定应写入记录");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn launch_job_cancel_via_registry_stops_process() {
        let reg = Arc::new(ToolJobRegistry::new(
            crate::cm_internal::tool_jobs::types::JobLimits {
                max_concurrent: 4,
                max_queued: 32,
                ttl: Duration::from_secs(3600),
                grace: Duration::from_secs(60),
                max_entries: 128,
            },
        ));
        let id = reg
            .register(std::path::PathBuf::from("/ws"), None)
            .expect("register");
        reg.try_start().expect("start");
        let mut cmd = Command::new("sleep");
        cmd.arg("60");
        let spawn = JobSpawn {
            command: cmd,
            wall: Duration::from_secs(30),
            max_output_len: 1024,
        };
        let wc: Arc<dyn Fn(&JobOutcome) -> bool + Send + Sync> = Arc::new(|_| false);
        let handle = launch_job(Arc::clone(&reg), id.clone(), spawn, wc);
        // 等 worker 真正进入等待循环后经注册表取消（验证 cancel_flag 接线）。
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(reg.cancel(&id), crate::cm_internal::tool_jobs::registry::CancelOutcome::Cancelled);
        handle.await.expect("join");
        let rec = reg.get(&id).expect("record");
        assert_eq!(rec.status, JobStatus::Cancelled);
        assert_eq!(rec.outcome.as_ref().expect("out").error_code.as_deref(), Some("cancelled"));
    }
}
