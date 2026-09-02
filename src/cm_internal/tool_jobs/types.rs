//! 后台工具任务：状态、记录与限额类型。
//!
//! 契约见 `docs/design/background_tool_jobs_contract.md`。状态机：
//! `queued → running → succeeded | failed | cancelled | timed_out`；**`expired` 不是持久状态**（TTL+宽限到期即删除记录，轮询得 `410`）。
//!
//! 实时输出流（环形缓冲）契约见 `docs/design/background_tool_jobs_output_streaming_contract.md`。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::cm_tools::subprocess_session::SessionStream;

/// 环形输出缓冲保留的元素条数硬上限（防海量微块；默认容量下平均开销约 32 B/元素）。
pub const MAX_OUTPUT_ITEMS: usize = 8192;
/// `GET /tools/jobs/{id}/output` 单次响应最多返回的元素条数（防大 JSON）。
pub const MAX_ITEMS_PER_RESPONSE: usize = 500;

/// job 实时输出缓冲中的一条记录（`seq` 全局单调；裁剪只丢最旧）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputEvent {
    pub seq: u64,
    pub stream: SessionStream,
    pub text: String,
}

/// [`JobOutputLog::read`] 的结果（不含 `eof`——由注册表结合 job 终态判定）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputLogRead {
    /// 自游标起的保留元素（升序，至多 [`MAX_ITEMS_PER_RESPONSE`] 条）。
    pub items: Vec<OutputEvent>,
    /// 下次请求应携带的游标（= 最后一条 `seq`+1；无 item 时为本次起点）。
    pub next_cursor: u64,
    /// 请求游标早于缓冲最早保留 seq（有数据被环形丢弃，本次从最早可用重放）。
    pub truncated: bool,
}

/// job 级环形输出缓冲（尾部保留；`seq` 单调不回填）。
///
/// - 字节上限与元素条数上限双保险，超限**丢最旧**（至少保留 1 条，不拆元素）。
/// - 空缓冲 ⇔ `written == 0`（裁剪恒保留 ≥1 条），`read` 的"最早可用"= 下一条 `seq`（`written+1`）。
#[derive(Debug, Default)]
pub struct JobOutputLog {
    events: VecDeque<OutputEvent>,
    /// 已写入元素总数（= 下一条 `seq` - 1）。
    written: u64,
    /// 保留元素文本字节合计（stdout/stderr 合并，字节上限裁剪用）。
    bytes: usize,
    stdout_bytes: usize,
    stderr_bytes: usize,
}

impl JobOutputLog {
    /// 追加一条输出；返回本次被环形裁剪丢弃的**元素条数**（`0` = 未丢弃）。
    pub fn push(&mut self, stream: SessionStream, text: &str, max_bytes: usize) -> usize {
        if text.is_empty() {
            return 0;
        }
        self.written += 1;
        let len = text.len();
        self.bytes += len;
        match stream {
            SessionStream::Stdout => self.stdout_bytes += len,
            SessionStream::Stderr => self.stderr_bytes += len,
        }
        self.events.push_back(OutputEvent {
            seq: self.written,
            stream,
            text: text.to_string(),
        });
        let mut dropped = 0;
        while (self.bytes > max_bytes || self.events.len() > MAX_OUTPUT_ITEMS)
            && self.events.len() > 1
        {
            self.pop_front_inner();
            dropped += 1;
        }
        dropped
    }

    /// 终态裁剪（**内存优先**）：自最旧起丢弃，直到「stdout、stderr 各自 ≤ `max_per_stream`」
    /// 或仅剩 1 条。返回移除条数。
    ///
    /// 语义说明：环形缓冲按合并时序裁剪，只能丢头部——要压掉某流尾部的超限部分，必须一并
    /// 丢弃其前的**另一流**事件（含已达标流），直至可行裁剪点；因此"各流各自 ≤ cap"为**尽力**：
    /// - 末尾单条超大元素不拆（允许该流单独越界，单条 ≤ 8 KiB 量级）；
    /// - 无法同时满足双流上限时以内存为优先，可能把已达标流一并丢光（仅剩 1 条兜底）。
    pub fn terminal_trim(&mut self, max_per_stream: usize) -> usize {
        let mut removed = 0;
        while (self.stdout_bytes > max_per_stream || self.stderr_bytes > max_per_stream)
            && self.events.len() > 1
        {
            self.pop_front_inner();
            removed += 1;
        }
        removed
    }

    /// 返回自 `cursor`（含）起的保留元素。
    ///
    /// - `cursor=None` / `0` → 从最早可用起（不标 `truncated`）；
    /// - `cursor` 早于最早保留 seq → `truncated=true`，从最早可用重放；
    /// - 单流内/合并单序均为升序、跨响应不重不漏（除非 `truncated`）。
    pub fn read(&self, cursor: Option<u64>) -> OutputLogRead {
        let requested = cursor.unwrap_or(0);
        let earliest = self
            .events
            .front()
            .map(|e| e.seq)
            .unwrap_or(self.written + 1);
        let truncated = requested != 0
            && self
                .events
                .front()
                .is_some_and(|e| requested < e.seq);
        let start = requested.max(earliest);
        let mut items = Vec::new();
        for ev in &self.events {
            if ev.seq >= start {
                items.push(ev.clone());
                if items.len() >= MAX_ITEMS_PER_RESPONSE {
                    break;
                }
            }
        }
        let next_cursor = items.last().map_or(start, |e| e.seq + 1);
        OutputLogRead {
            items,
            next_cursor,
            truncated,
        }
    }

    /// 已写入元素总数（`eof` 判定用）。
    #[must_use]
    pub fn written(&self) -> u64 {
        self.written
    }

    /// 当前保留元素的文本字节合计（观测/统计用；单条超大元素时可能略超上限）。
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }

    /// 丢弃最旧一条并回扣计数。
    fn pop_front_inner(&mut self) {
        if let Some(front) = self.events.pop_front() {
            let len = front.text.len();
            self.bytes = self.bytes.saturating_sub(len);
            match front.stream {
                SessionStream::Stdout => self.stdout_bytes = self.stdout_bytes.saturating_sub(len),
                SessionStream::Stderr => self.stderr_bytes = self.stderr_bytes.saturating_sub(len),
            }
        }
    }
}

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
    /// 每 job 环形输出缓冲字节上限（超限丢最旧；终态裁剪为各流尾部 ≤ `command_max_output_len`）。
    /// 默认 `262144`（256 KiB），范围 4096–16777216（契约 `background_tool_jobs_output_streaming_contract.md` §4）。
    pub output_buffer_bytes: usize,
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
            output_buffer_bytes: cfg.tool_registry_background_job_output_buffer_bytes as usize,
        }
    }
}
