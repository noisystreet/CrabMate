//! 进程内可观测性：`tracing` span 与 Web 聊天任务关联字段（`job_id`、`conversation_id` 等）。

use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tracing::Span;
use tracing_log::LogTracer;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::time::LocalTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// 与 HTTP **`x-stream-job-id`** / SSE **`sse_capabilities.job_id`** 对齐的单调 **`job_id`**（根 span 字段名）。
pub const CHAT_TURN_SPAN_NAME: &str = "chat_turn";

/// `chat_turn` span 内 `conversation_id` 字段最大 Unicode 标量（超出则 `…(truncated)`，避免每行 INFO 被会话 id 撑满）。
pub const CHAT_TURN_CONVERSATION_ID_FIELD_MAX_CHARS: usize = 56;
/// `chat_turn` span 内 **`tool_call_id`** 展示串（**`#序号·…尾部`**）里尾部片段的最大 Unicode 标量数（超出则只保留尾部并加 **`…`** 前缀）。
pub const CHAT_TURN_TOOL_CALL_LOG_TAIL_MAX_CHARS: usize = 12;

static LOGGING_INIT: OnceLock<Result<(), String>> = OnceLock::new();

/// 同条日志写 stderr + 可选文件（与历史 `env_logger` 双写语义一致）。
struct StderrAndFile {
    stderr: io::Stderr,
    file: std::fs::File,
}

impl Write for StderrAndFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stderr.write_all(buf)?;
        self.file.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stderr.flush()?;
        self.file.flush()
    }
}

/// `tracing-subscriber` 的 `MakeWriter` 需要返回 `Write`；用 `&mut self` 写时借内部 `Mutex`。
struct StderrFilePipeWriter(Arc<Mutex<StderrAndFile>>);

impl Write for StderrFilePipeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut g = self
            .0
            .lock()
            .map_err(|_| io::Error::other("日志管道互斥锁已中毒（poisoned）"))?;
        g.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut g = self
            .0
            .lock()
            .map_err(|_| io::Error::other("日志管道互斥锁已中毒（poisoned）"))?;
        g.flush()
    }
}

fn open_log_append(path: &Path) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

fn agent_log_json_truthy(s: &str) -> Option<bool> {
    let v = s.trim().to_ascii_lowercase();
    if matches!(v.as_str(), "1" | "true" | "yes" | "on") {
        Some(true)
    } else if matches!(v.as_str(), "0" | "false" | "no" | "off") {
        Some(false)
    } else {
        None
    }
}

fn default_env_filter(quiet_cli_default: bool, log_file: Option<&Path>) -> String {
    if std::env::var_os("RUST_LOG").is_some() {
        return String::new();
    }
    let base = if log_file.is_some() {
        "info"
    } else if quiet_cli_default {
        "warn"
    } else {
        "info"
    };
    format!("{base},tokei=error")
}

/// 初始化 **`tracing` subscriber** + **`tracing-log`**（桥接既有 `log::` 调用）。
///
/// 与历史 `init_logging` 行为对齐：**`RUST_LOG`** 优先；未设置时按 `quiet_cli_default` / `--log` 给默认过滤器；默认均带 **`tokei=error`**。
///
/// 设 **`CM_LOG_JSON=1`**（或 **`true`** / **`yes`** / **`on`**）时，日志行为 **JSON 行**（便于 `jq` / 日志平台）；否则为紧凑人类可读格式（含 span 字段上下文）。
///
/// 行首时间戳默认**本机本地时区**（`tracing_subscriber::fmt::time::LocalTime::rfc_3339`，RFC3339 且带与 UTC 的偏移），由系统 `TZ` / `/etc/localtime` 决定。
pub fn init_tracing_subscriber(log_file: Option<&Path>, quiet_cli_default: bool) -> io::Result<()> {
    let result = LOGGING_INIT.get_or_init(|| {
        let filter_str = default_env_filter(quiet_cli_default, log_file);
        let env_filter = if std::env::var_os("RUST_LOG").is_some() {
            EnvFilter::try_from_default_env().map_err(|e| e.to_string())?
        } else {
            EnvFilter::new(&filter_str)
        };

        let json_logs = std::env::var("CM_LOG_JSON")
            .ok()
            .as_deref()
            .and_then(agent_log_json_truthy)
            .unwrap_or(false);

        let ansi_stderr = std::io::stderr().is_terminal() && log_file.is_none();

        match log_file {
            None => {
                if json_logs {
                    tracing_subscriber::registry()
                        .with(env_filter)
                        .with(
                            tracing_subscriber::fmt::layer()
                                .json()
                                .with_target(true)
                                .with_timer(LocalTime::rfc_3339())
                                .with_writer(std::io::stderr),
                        )
                        .init();
                } else {
                    tracing_subscriber::registry()
                        .with(env_filter)
                        .with(
                            tracing_subscriber::fmt::layer()
                                .with_target(true)
                                .with_timer(LocalTime::rfc_3339())
                                .with_ansi(ansi_stderr)
                                .compact()
                                .with_writer(std::io::stderr),
                        )
                        .init();
                }
            }
            Some(path) => {
                let f = open_log_append(path)
                    .map_err(|e| format!("无法打开日志文件 {}: {e}", path.display()))?;
                let pipe = Arc::new(Mutex::new(StderrAndFile {
                    stderr: io::stderr(),
                    file: f,
                }));
                if json_logs {
                    let wj = Arc::clone(&pipe);
                    tracing_subscriber::registry()
                        .with(env_filter)
                        .with(
                            tracing_subscriber::fmt::layer()
                                .json()
                                .with_target(true)
                                .with_timer(LocalTime::rfc_3339())
                                .with_writer(move || StderrFilePipeWriter(Arc::clone(&wj))),
                        )
                        .init();
                } else {
                    let wc = Arc::clone(&pipe);
                    tracing_subscriber::registry()
                        .with(env_filter)
                        .with(
                            tracing_subscriber::fmt::layer()
                                .with_target(true)
                                .with_timer(LocalTime::rfc_3339())
                                .with_ansi(false)
                                .compact()
                                .with_writer(move || StderrFilePipeWriter(Arc::clone(&wc))),
                        )
                        .init();
                }
            }
        }

        LogTracer::init().map_err(|e| e.to_string())?;
        Ok(())
    });

    match result {
        Ok(()) => Ok(()),
        Err(s) => Err(io::Error::other(s.clone())),
    }
}

/// Web 单条 `/chat*` 任务根 span：**`job_id`** 与 HTTP **`x-stream-job-id`** / SSE **`job_id`** 一致；**`conversation_id`** 为截断预览（完整 id 仍由业务层与会话存储持有）。
pub fn chat_turn_span(job_id: u64, conversation_id: &str) -> Span {
    let conversation_id_field = crate::redact::preview_chars(
        conversation_id.trim(),
        CHAT_TURN_CONVERSATION_ID_FIELD_MAX_CHARS,
    );
    tracing::info_span!(
        CHAT_TURN_SPAN_NAME,
        job_id = job_id,
        conversation_id = %conversation_id_field,
        conversation_id_len = conversation_id.trim().chars().count(),
        outer_loop_iteration = tracing::field::Empty,
        tool_call_seq = tracing::field::Empty,
        tool_call_id = tracing::field::Empty,
    )
}

fn tool_call_id_suffix_for_log(raw: &str) -> String {
    let t = raw.trim();
    let cs: Vec<char> = t.chars().collect();
    if t.is_empty() {
        return "?".to_string();
    }
    if cs.len() <= CHAT_TURN_TOOL_CALL_LOG_TAIL_MAX_CHARS {
        return t.to_string();
    }
    let start = cs.len() - CHAT_TURN_TOOL_CALL_LOG_TAIL_MAX_CHARS;
    format!("…{}", cs[start..].iter().collect::<String>())
}

pub fn record_outer_loop_iteration(span: &Span, iteration: u32) {
    span.record("outer_loop_iteration", iteration);
}

/// 将 **`tool_call_seq`**（本轮工具调用单调序号，从 1 起）与 **`tool_call_id`**（`#n·…尾部`，便于扫读）写入 `chat_turn` span。
/// 返回与写入字段一致的展示标签，供 **`parallel_tool`** 子 span 等同款缩短，避免嵌套 span 仍打出完整上游 id。
pub fn record_tool_call_seq_and_label(span: &Span, seq: u32, raw_tool_call_id: &str) -> String {
    let label = format!("#{seq}·{}", tool_call_id_suffix_for_log(raw_tool_call_id));
    span.record("tool_call_seq", seq);
    // `display` + `clone`：确保格式化层持有副本（勿在 `label` 析构后仍借用 `as_str()`）。
    span.record("tool_call_id", tracing::field::display(label.clone()));
    label
}

/// Web `/chat*` 单任务：`job_id` / `conversation_id` 根 span + 可递增的外层轮次；工具日志前更新 **`tool_call_seq`** / **`tool_call_id`**（展示标签，协议层仍以原始 id 为准）。
#[derive(Debug)]
pub struct TracingChatTurn {
    /// 与 HTTP **`x-stream-job-id`**、SSE **`sse_capabilities.job_id`** 一致（CLI 等无 Web 任务时为占位，通常不用于观测）。
    pub job_id: u64,
    pub span: Span,
    outer_iteration: AtomicU32,
    tool_call_seq: AtomicU32,
}

impl TracingChatTurn {
    pub fn new(job_id: u64, conversation_id: &str) -> Arc<Self> {
        Arc::new(Self {
            job_id,
            span: chat_turn_span(job_id, conversation_id),
            outer_iteration: AtomicU32::new(0),
            tool_call_seq: AtomicU32::new(0),
        })
    }

    /// 每进入一次 `run_agent_outer_loop` 主循环体调用（从 **1** 递增）。
    pub fn on_outer_loop_iteration(&self) {
        let v = self.outer_iteration.fetch_add(1, Ordering::Relaxed) + 1;
        record_outer_loop_iteration(&self.span, v);
    }

    /// 返回与 **`chat_turn`** / **`parallel_tool`** 字段一致的短标签（`#序号·…尾部`）。
    pub fn record_tool_call_id_for_log(&self, tool_call_id: &str) -> String {
        let seq = self.tool_call_seq.fetch_add(1, Ordering::Relaxed) + 1;
        record_tool_call_seq_and_label(&self.span, seq, tool_call_id)
    }
}

#[cfg(test)]
mod tool_call_log_tests {
    use super::tool_call_id_suffix_for_log;

    #[test]
    fn suffix_short_or_tail() {
        assert_eq!(tool_call_id_suffix_for_log(""), "?");
        assert_eq!(tool_call_id_suffix_for_log("ab"), "ab");
        let long = "call_00_BkY0VJ0b9ntl4djhm6ji1935";
        assert!(tool_call_id_suffix_for_log(long).starts_with('…'));
        assert!(tool_call_id_suffix_for_log(long).contains("ji1935"));
    }
}
