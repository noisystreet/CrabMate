//! 后台工具任务：状态、记录与限额类型。
//!
//! 契约见 `docs/design/background_tool_jobs_contract.md`。状态机：
//! `queued → running → succeeded | failed | cancelled | timed_out`；**`expired` 不是持久状态**（TTL+宽限到期即删除记录，轮询得 `410`）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// 任务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

impl JobStatus {
    #[must_use]
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Queued | Self::Running)
    }

    /// 轮询/SSE 响应中的稳定字符串取值（契约 §3.1）。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
        }
    }
}

/// 一次后台执行的运行结果（worker 产出）。
///
/// `workspace_changed` 由调用方按输出判定（`run_command` 复用 `is_compile_command_success`），
/// 经 [`super::registry::ToolJobRegistry::complete`] 写入记录。
#[derive(Debug, Clone)]
pub struct JobOutcome {
    /// 终态之一：`Succeeded` / `Failed` / `TimedOut` / `Cancelled`。
    pub status: JobStatus,
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// `timeout` / `cancelled` / `internal`（worker panic）/ `spawn_failed` / `wait_failed`。
    pub error_code: Option<String>,
    pub failure_category: Option<String>,
}

impl JobOutcome {
    #[must_use]
    pub fn failed(code: &str) -> Self {
        Self {
            status: JobStatus::Failed,
            exit_code: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            error_code: Some(code.to_string()),
            failure_category: None,
        }
    }
}

/// 一次后台执行的参数（命令已通过白名单/路径/审批）。
///
/// 纯数据（`Clone`），供排队任务在领取时重建 `Command`；取消信号不在其中：
/// 由注册表持有一份（`JobRecord.cancel_flag`），`launch_job` 启动时从注册表取同一次
/// `Arc<AtomicBool>` 传入，`registry.cancel()` 才能命中同一信号。
#[derive(Debug, Clone)]
pub struct JobSpawn {
    /// 可执行文件路径或命令名（`Command::new` 的入参）。
    pub program: String,
    pub args: Vec<String>,
    /// 进程工作目录。
    pub cwd: PathBuf,
    /// 额外环境变量（如 `GH_TOKEN`；不含 `PATH` 等继承项）。
    pub extra_env: Vec<(String, String)>,
    /// 墙钟；超时对进程组 SIGTERM→SIGKILL。
    pub wall: Duration,
    /// 输出截断上限（复用 `command_max_output_len`）。
    pub max_output_len: usize,
}

impl JobSpawn {
    /// 重建可 spawn 的 `Command`（进程组/环境由 worker 再装配）。
    #[must_use]
    pub fn to_command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args).current_dir(&self.cwd);
        if !self.extra_env.is_empty() {
            cmd.envs(self.extra_env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        }
        cmd
    }

    #[cfg(test)]
    pub(crate) fn from_command(cmd: &mut Command, wall: Duration, max_output_len: usize) -> Self {
        let program = cmd.get_program().to_string_lossy().into_owned();
        let args = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        let cwd = cmd.get_current_dir().unwrap_or(Path::new(".")).to_path_buf();
        let extra_env = Vec::new();
        Self {
            program,
            args,
            cwd,
            extra_env,
            wall,
            max_output_len,
        }
    }
}

/// 注册表条目。
#[derive(Debug, Clone)]
pub struct JobRecord {
    /// `tooljob_` + 32 hex（随机不透明，不可枚举）。
    pub id: String,
    /// 创建时的 workspace（轮询端点可选 `X-Workspace-Root` 归属校验依据）。
    pub workspace: PathBuf,
    /// 发起它的 LLM turn `job_id`（日志关联；无则 `None`）。
    pub source_turn_job_id: Option<u64>,
    pub status: JobStatus,
    pub created_at: SystemTime,
    pub finished_at: Option<SystemTime>,
    /// `running` 时收到取消请求的标记（worker 观察 `AtomicBool` 后完成转移）。
    pub cancel_requested: bool,
    /// worker 取消信号（`register` 时创建；`cancel()` 置位，`launch_job` 传入 `wait_child_session`）。
    pub cancel_flag: Arc<AtomicBool>,
    /// 领取（`queued → running`）后用于启动的纯数据参数；注册时必填。
    pub spawn: JobSpawn,
    /// 发起时的 `run_command` 参数 JSON（终态 `workspace_changed` 判定与日志关联用）。
    pub args_json: String,
    pub workspace_changed: bool,
    pub outcome: Option<JobOutcome>,
}

/// 注册表限额（来自 `[tool_registry]` 配置，见 `background_tool_jobs_contract.md` §6）。
#[derive(Debug, Clone, Copy)]
pub struct JobLimits {
    /// 同时运行上限（超出进入 `queued`，FIFO）。
    pub max_concurrent: usize,
    /// 排队上限（`0` = 满并发即拒绝创建）。
    pub max_queued: usize,
    /// 自**创建**起算的保留时长。
    pub ttl: Duration,
    /// 终态后再保留的宽限（避免"刚完成即被清"）。
    pub grace: Duration,
    /// 条目上限；**仅淘汰终态**条目。
    pub max_entries: usize,
}

impl JobLimits {
    /// 由 `[tool_registry]` 配置（1.1 已烘焙默认值）构造。
    #[must_use]
    pub fn from_config(cfg: &crate::cm_config::ToolRegistryPolicyConfig) -> Self {
        Self {
            max_concurrent: cfg.tool_registry_background_job_max_concurrent as usize,
            max_queued: cfg.tool_registry_background_job_max_queued as usize,
            ttl: Duration::from_secs(cfg.tool_registry_background_job_ttl_secs),
            grace: Duration::from_secs(cfg.tool_registry_background_job_result_grace_secs),
            max_entries: cfg.tool_registry_background_job_max_entries as usize,
        }
    }
}
